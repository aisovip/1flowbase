//! Append-only restoration of the effective latest recovery state. Immutable
//! provenance is retained, so restoration creates a new head identity.
use super::*;

pub(super) struct RestorationHead {
    content: Value,
    parent: Option<Uuid>,
}

pub(super) async fn original_head(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    receipt: &ResponsesHistoryMigrationReceipt,
    current: &ResponsesHistoryFlowSnapshot,
) -> Result<Option<RestorationHead>> {
    if receipt.before_latest_context == receipt.after_latest_context {
        return Ok(None);
    }
    if let Some(identity) = &receipt.before_latest_context {
        let old = contexts::context(tx, &receipt.flow, identity.context_version_id)
            .await?
            .entry;
        anyhow::ensure!(
            old.identity == *identity,
            "responses_history_original_latest_changed"
        );
        return Ok(Some(RestorationHead {
            content: old.content,
            parent: old.previous_projection_id,
        }));
    }
    // Absence of an implicit max cannot be restored without deleting immutable
    // audit nodes. Preserve effective data using the original latest inline CP.
    let anchor = receipt
        .checkpoints
        .last()
        .ok_or_else(|| anyhow!("responses_history_legacy_rollback_anchor_missing"))?;
    anyhow::ensure!(
        current
            .checkpoints
            .last()
            .is_some_and(|c| c.checkpoint_id == anchor.checkpoint_id)
            && anchor.before_context.is_none()
            && checkpoints::locator_context(&anchor.before_locator_payload)?.is_none(),
        "responses_history_legacy_rollback_anchor_invalid"
    );
    let mut pool = anchor
        .before_variable_snapshot
        .as_object()
        .cloned()
        .ok_or_else(|| anyhow!("responses_history_legacy_rollback_pool_invalid"))?;
    anyhow::ensure!(
        !(pool.len() == 1 && pool.contains_key("__runtime_recovery_context")),
        "responses_history_legacy_rollback_anchor_missing"
    );
    pool.remove("__runtime_recovery_context");
    Ok(Some(RestorationHead {
        content: json!({"format":"runtime_snapshot_v1","variable_pool":pool}),
        parent: None,
    }))
}

pub(super) async fn already_restored(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    receipt: &ResponsesHistoryMigrationReceipt,
    current: &ResponsesHistoryFlowSnapshot,
    head: &Option<RestorationHead>,
) -> Result<bool> {
    let head_matches = if let Some(head) = head {
        let Some(latest) = current.latest_context_version_id else {
            return Ok(false);
        };
        let actual = contexts::context(tx, &receipt.flow, latest).await?;
        let after = receipt
            .after_latest_context
            .as_ref()
            .ok_or_else(|| anyhow!("responses_history_receipt_latest_invalid"))?;
        let after = contexts::context(tx, &receipt.flow, after.context_version_id)
            .await?
            .entry;
        actual.entry.sequence > after.sequence
            && actual.entry.content == head.content
            && actual.entry.previous_projection_id == head.parent
            && actual.transition_kind.as_deref() == Some("append")
            && actual.transition_actor.as_deref() == Some("host")
    } else {
        snapshot_latest(current) == receipt.before_latest_context
    };
    if !head_matches {
        return Ok(false);
    }
    for item in &receipt.documents {
        if documents::read_current(tx, &receipt.flow, item.change.owner, true).await?
            != item.change.before
            || documents::read_shadow(tx, &receipt.flow, item.change.owner).await?
                != item.before_shadow
        {
            return Ok(false);
        }
        if let Some(binding) = &item.before_shadow {
            documents::verify_shadow_content(tx, &receipt.flow, binding, &item.change.before)
                .await?;
        }
    }
    let cp_index: HashMap<_, _> = current
        .checkpoints
        .iter()
        .map(|c| (c.checkpoint_id, c))
        .collect();
    for item in &receipt.checkpoints {
        let Some(actual) = cp_index.get(&item.checkpoint_id) else {
            return Ok(false);
        };
        if actual.locator_payload != item.before_locator_payload
            || actual.variable_snapshot != item.before_variable_snapshot
            || actual.context != item.before_context
            || checkpoints::read_shadow(tx, &receipt.flow, item.checkpoint_id).await?
                != item.before_shadow
        {
            return Ok(false);
        }
        if let Some(binding) = &item.before_shadow {
            documents::verify_shadow_content(
                tx,
                &receipt.flow,
                binding,
                &item.before_variable_snapshot,
            )
            .await?;
        }
    }
    for item in &receipt.continuations {
        if !current
            .continuations
            .iter()
            .any(|slot| slot == &item.before)
        {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) async fn append_head(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    flow: &ResponsesHistoryFlowKey,
    head: Option<RestorationHead>,
) -> Result<()> {
    if let Some(head) = head {
        contexts::append(tx, flow, &head.content, head.parent).await?;
    }
    Ok(())
}

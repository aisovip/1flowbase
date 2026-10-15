use super::*;
use std::collections::HashMap;
const MARKER: &str = "__runtime_recovery_context";

pub(super) fn locator_context(locator: &Value) -> Result<Option<Uuid>> {
    let object = locator
        .as_object()
        .ok_or_else(|| anyhow!("responses_history_checkpoint_locator_invalid"))?;
    object
        .get("context_version_id")
        .map(|id| {
            Uuid::parse_str(
                id.as_str()
                    .ok_or_else(|| anyhow!("responses_history_checkpoint_context_invalid"))?,
            )
            .map_err(|_| anyhow!("responses_history_checkpoint_context_invalid"))
        })
        .transpose()
}

pub(super) fn inline_content(change: &ResponsesHistoryCheckpointChange) -> Result<Option<Value>> {
    anyhow::ensure!(
        change.before_context.is_none() == change.legacy_inline_variable_pool.is_some(),
        "responses_history_checkpoint_representation_invalid"
    );
    change
        .legacy_inline_variable_pool
        .as_ref()
        .map(|pool| {
            let mut pool = pool
                .as_object()
                .cloned()
                .ok_or_else(|| anyhow!("responses_history_checkpoint_pool_invalid"))?;
            pool.remove(MARKER);
            Ok(json!({"format":"runtime_snapshot_v1","variable_pool":pool}))
        })
        .transpose()
}

fn projected(
    change: &ResponsesHistoryCheckpointChange,
    id: Uuid,
    sequence: i64,
) -> Result<(Value, Value)> {
    let mut locator = change.before_locator_payload.clone();
    anyhow::ensure!(
        locator.is_object() && change.before_variable_snapshot.is_object(),
        "responses_history_checkpoint_projection_invalid"
    );
    locator["context_version_id"] = json!(id);
    Ok((
        locator,
        json!({MARKER:{"context_version_id":id,"sequence":sequence,"parent_context_version_id":null}}),
    ))
}

pub(super) async fn load_all(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    flow: &ResponsesHistoryFlowKey,
    lock: bool,
) -> Result<Vec<ResponsesHistoryCheckpoint>> {
    let suffix = if lock { "for update of c" } else { "" };
    let rows=sqlx::query(&format!("select c.id,c.scope_id,c.node_run_id,(n.flow_run_id=$1 and n.scope_id=$2) as node_valid,runtime_original_json(c.locator_payload,c.raw_json_payloads,'locator_payload') as locator_payload,runtime_original_json(c.variable_snapshot,c.raw_json_payloads,'variable_snapshot') as variable_snapshot from flow_run_checkpoints c left join node_runs n on n.id=c.node_run_id where c.flow_run_id=$1 order by c.created_at,c.id {suffix}"))
        .bind(flow.flow_run_id).bind(flow.scope_id).fetch_all(&mut **tx).await?;
    let mut result = Vec::with_capacity(rows.len());
    for row in rows {
        let id: Uuid = row.try_get("id")?;
        anyhow::ensure!(
            row.try_get::<Uuid, _>("scope_id")? == flow.scope_id
                && (row.try_get::<Option<Uuid>, _>("node_run_id")?.is_none()
                    || row.try_get::<Option<bool>, _>("node_valid")? == Some(true)),
            "responses_history_checkpoint_owner_mismatch"
        );
        if lock {
            sqlx::query("select pg_advisory_xact_lock(hashtextextended($1,0))")
                .bind(format!("flow_run_checkpoints:variable_snapshot:{id}"))
                .execute(&mut **tx)
                .await?;
        }
        let locator_payload: Value = row.try_get("locator_payload")?;
        locator_context(&locator_payload)?;
        result.push(ResponsesHistoryCheckpoint {
            checkpoint_id: id,
            locator_payload,
            variable_snapshot: row.try_get("variable_snapshot")?,
            context: None,
        });
    }
    Ok(result)
}

pub(super) async fn read_shadow(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    flow: &ResponsesHistoryFlowKey,
    id: Uuid,
) -> Result<Option<ResponsesHistoryShadowBinding>> {
    let row=sqlx::query("select id,scope_id,application_id,flow_run_id,source_hash,source_byte_size,canonical_content_id,context_version_id from runtime_legacy_shadow_rows where source_table='flow_run_checkpoints' and source_column='variable_snapshot' and source_row_id=$1 for update")
        .bind(id).fetch_optional(&mut **tx).await?;
    row.map(|row| {
        anyhow::ensure!(
            row.try_get::<Uuid, _>("scope_id")? == flow.scope_id
                && row.try_get::<Uuid, _>("application_id")? == flow.application_id
                && row.try_get::<Uuid, _>("flow_run_id")? == flow.flow_run_id,
            "responses_history_checkpoint_shadow_owner_mismatch"
        );
        Ok(ResponsesHistoryShadowBinding {
            shadow_row_id: row.try_get("id")?,
            source_hash: row.try_get("source_hash")?,
            source_byte_size: row.try_get("source_byte_size")?,
            canonical_content_id: row.try_get("canonical_content_id")?,
            context_version_id: row.try_get("context_version_id")?,
        })
    })
    .transpose()
}

pub(super) async fn verify_current_shadow(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    flow: &ResponsesHistoryFlowKey,
    current: &ResponsesHistoryCheckpoint,
) -> Result<Option<ResponsesHistoryShadowBinding>> {
    let shadow = read_shadow(tx, flow, current.checkpoint_id).await?;
    if let Some(binding) = &shadow {
        documents::verify_shadow_content(tx, flow, binding, &current.variable_snapshot).await?;
    }
    Ok(shadow)
}

async fn write_checkpoint(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    flow: &ResponsesHistoryFlowKey,
    id: Uuid,
    locator: &Value,
    snapshot: &Value,
) -> Result<()> {
    let updated=sqlx::query("update flow_run_checkpoints set locator_payload=($2::jsonb->0),variable_snapshot=($3::jsonb->0),raw_json_payloads=(raw_json_payloads-'locator_payload'-'variable_snapshot') || jsonb_strip_nulls(jsonb_build_object('locator_payload',($2::jsonb->1),'variable_snapshot',($3::jsonb->1))) where id=$1 and flow_run_id=$4 and scope_id=$5")
        .bind(id).bind(lossless_json_parameter(locator)).bind(lossless_json_parameter(snapshot)).bind(flow.flow_run_id).bind(flow.scope_id).execute(&mut **tx).await?;
    anyhow::ensure!(
        updated.rows_affected() == 1,
        "responses_history_checkpoint_write_conflict"
    );
    Ok(())
}

pub(super) fn is_before(
    actual: &ResponsesHistoryCheckpoint,
    change: &ResponsesHistoryCheckpointChange,
) -> bool {
    actual.locator_payload == change.before_locator_payload
        && actual.variable_snapshot == change.before_variable_snapshot
        && actual.context == change.before_context
}

pub(super) fn has_after_projection(
    actual: &ResponsesHistoryCheckpoint,
    change: &ResponsesHistoryCheckpointChange,
    entry: &ResponsesHistoryContextLineageEntry,
) -> Result<bool> {
    let (locator, snapshot) = projected(change, entry.identity.context_version_id, entry.sequence)?;
    Ok(actual.locator_payload == locator
        && actual.variable_snapshot == snapshot
        && actual.context.as_ref() == Some(&entry.identity)
        && change
            .before_context
            .as_ref()
            .is_none_or(|old| old.context_version_id != entry.identity.context_version_id))
}

/// Inline nodes are the only newly introduced full snapshots. Existing DAG nodes
/// are copied separately, once per old ID, preserving their actual encoding.
pub(super) async fn append_inline(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    flow: &ResponsesHistoryFlowKey,
    changes: &[ResponsesHistoryCheckpointChange],
) -> Result<HashMap<Uuid, ResponsesHistoryContextLineageEntry>> {
    let mut result = HashMap::new();
    for change in changes {
        if let Some(content) = inline_content(change)? {
            result.insert(
                change.checkpoint_id,
                contexts::append(tx, flow, &content, None).await?,
            );
        }
    }
    Ok(result)
}

pub(super) async fn apply(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    flow: &ResponsesHistoryFlowKey,
    changes: &[ResponsesHistoryCheckpointChange],
    mapping: &HashMap<Uuid, ResponsesHistoryContextLineageEntry>,
    inline: &HashMap<Uuid, ResponsesHistoryContextLineageEntry>,
) -> Result<Vec<ResponsesHistoryCheckpointReceipt>> {
    let mut result = Vec::with_capacity(changes.len());
    for change in changes {
        let after = if let Some(before) = &change.before_context {
            mapping.get(&before.context_version_id)
        } else {
            inline.get(&change.checkpoint_id)
        }
        .ok_or_else(|| anyhow!("responses_history_checkpoint_context_mapping_missing"))?;
        let (after_locator_payload, after_variable_snapshot) =
            projected(change, after.identity.context_version_id, after.sequence)?;
        let before_shadow = read_shadow(tx, flow, change.checkpoint_id).await?;
        let mut after_shadow = before_shadow.clone();
        if let Some(binding) = &mut after_shadow {
            let (id, hash, size) = put_canonical_runtime_content_in_transaction(
                tx,
                flow.scope_id,
                flow.application_id,
                &after_variable_snapshot,
            )
            .await?;
            binding.source_hash = hash;
            binding.source_byte_size = size;
            binding.canonical_content_id = id;
            binding.context_version_id = None;
            documents::write_shadow(tx, binding).await?;
        }
        write_checkpoint(
            tx,
            flow,
            change.checkpoint_id,
            &after_locator_payload,
            &after_variable_snapshot,
        )
        .await?;
        result.push(ResponsesHistoryCheckpointReceipt {
            checkpoint_id: change.checkpoint_id,
            before_locator_payload: change.before_locator_payload.clone(),
            before_variable_snapshot: change.before_variable_snapshot.clone(),
            before_context: change.before_context.clone(),
            after_locator_payload,
            after_variable_snapshot,
            after_context: after.identity.clone(),
            after_context_sequence: after.sequence,
            before_shadow,
            after_shadow,
        });
    }
    Ok(result)
}

pub(super) async fn rollback(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    flow: &ResponsesHistoryFlowKey,
    current: &[ResponsesHistoryCheckpoint],
    receipts: &[ResponsesHistoryCheckpointReceipt],
) -> Result<()> {
    let current_index: HashMap<_, _> = current.iter().map(|c| (c.checkpoint_id, c)).collect();
    for receipt in receipts {
        let checkpoint = current_index
            .get(&receipt.checkpoint_id)
            .ok_or_else(|| anyhow!("responses_history_checkpoint_missing"))?;
        anyhow::ensure!(
            checkpoint.locator_payload == receipt.after_locator_payload
                && checkpoint.variable_snapshot == receipt.after_variable_snapshot
                && checkpoint.context.as_ref() == Some(&receipt.after_context),
            "responses_history_checkpoint_rollback_conflict"
        );
        anyhow::ensure!(
            locator_context(&receipt.before_locator_payload)?
                == receipt
                    .before_context
                    .as_ref()
                    .map(|id| id.context_version_id),
            "responses_history_checkpoint_receipt_context_invalid"
        );
        let after = contexts::context(tx, flow, receipt.after_context.context_version_id)
            .await?
            .entry;
        anyhow::ensure!(
            after.identity == receipt.after_context
                && after.sequence == receipt.after_context_sequence,
            "responses_history_checkpoint_context_receipt_invalid"
        );
        if let Some(identity) = &receipt.before_context {
            anyhow::ensure!(
                contexts::context(tx, flow, identity.context_version_id)
                    .await?
                    .entry
                    .identity
                    == *identity,
                "responses_history_checkpoint_original_context_changed"
            );
        }
        let shadow = verify_current_shadow(tx, flow, checkpoint).await?;
        anyhow::ensure!(
            shadow == receipt.after_shadow
                && receipt.before_shadow.is_some() == receipt.after_shadow.is_some(),
            "responses_history_checkpoint_shadow_rollback_conflict"
        );
        if let (Some(before), Some(after)) = (&receipt.before_shadow, &receipt.after_shadow) {
            anyhow::ensure!(
                before.shadow_row_id == after.shadow_row_id,
                "responses_history_checkpoint_receipt_shadow_invalid"
            );
            documents::verify_shadow_content(tx, flow, before, &receipt.before_variable_snapshot)
                .await?;
        }
    }
    for receipt in receipts {
        write_checkpoint(
            tx,
            flow,
            receipt.checkpoint_id,
            &receipt.before_locator_payload,
            &receipt.before_variable_snapshot,
        )
        .await?;
        if let Some(binding) = &receipt.before_shadow {
            documents::write_shadow(tx, binding).await?;
        }
    }
    Ok(())
}

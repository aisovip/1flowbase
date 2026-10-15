use super::*;
use std::collections::{HashMap, HashSet};

pub(super) struct ContextRecord {
    pub entry: ResponsesHistoryContextLineageEntry,
    pub transition_kind: Option<String>,
    pub transition_actor: Option<String>,
}

pub(super) async fn context(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    flow: &ResponsesHistoryFlowKey,
    id: Uuid,
) -> Result<ContextRecord> {
    let row=sqlx::query("select p.id,p.previous_projection_id,p.context_sequence,p.actual_content_id,p.transition_kind,p.transition_actor,c.content_hash,c.byte_size,runtime_original_json(c.content,c.raw_json_payloads,'content') as content from runtime_context_projections p join runtime_canonical_contents c on c.id=p.actual_content_id and c.scope_id=p.scope_id and c.application_id=p.application_id where p.id=$1 and p.flow_run_id=$2 and p.scope_id=$3 and p.application_id=$4 and p.projection_kind='context_version'")
        .bind(id).bind(flow.flow_run_id).bind(flow.scope_id).bind(flow.application_id)
        .fetch_optional(&mut **tx).await?.ok_or_else(|| anyhow!("responses_history_checkpoint_context_owner_mismatch"))?;
    let content: Value = row.try_get("content")?;
    let hash: String = row.try_get("content_hash")?;
    let size: i64 = row.try_get("byte_size")?;
    anyhow::ensure!(
        canonical_runtime_json_identity(&content)? == (hash.clone(), size),
        "responses_history_checkpoint_content_integrity"
    );
    Ok(ContextRecord {
        entry: ResponsesHistoryContextLineageEntry {
            identity: ResponsesHistoryContextIdentity {
                context_version_id: id,
                actual_content_id: row.try_get("actual_content_id")?,
                content_hash: hash,
            },
            previous_projection_id: row.try_get("previous_projection_id")?,
            sequence: row.try_get("context_sequence")?,
            content,
        },
        transition_kind: row.try_get("transition_kind")?,
        transition_actor: row.try_get("transition_actor")?,
    })
}

/// Every reachable context and content is fetched once, independent of how many
/// checkpoints share its ancestry. Sequence monotonicity also rules out cycles.
pub(super) async fn load_dag(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    flow: &ResponsesHistoryFlowKey,
    roots: impl IntoIterator<Item = Uuid>,
) -> Result<Vec<ResponsesHistoryContextLineageEntry>> {
    let mut pending: Vec<_> = roots.into_iter().collect();
    let mut entries = HashMap::new();
    while let Some(id) = pending.pop() {
        if entries.contains_key(&id) {
            continue;
        }
        let entry = context(tx, flow, id).await?.entry;
        if let Some(parent) = entry.previous_projection_id {
            pending.push(parent);
        }
        entries.insert(id, entry);
    }
    for entry in entries.values() {
        if let Some(parent) = entry.previous_projection_id {
            anyhow::ensure!(
                entries
                    .get(&parent)
                    .is_some_and(|p| p.sequence < entry.sequence),
                "responses_history_checkpoint_sequence_invalid"
            );
        }
    }
    let mut result: Vec<_> = entries.into_values().collect();
    result.sort_by_key(|entry| (entry.sequence, entry.identity.context_version_id));
    Ok(result)
}

pub(super) async fn append(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    flow: &ResponsesHistoryFlowKey,
    content: &Value,
    parent: Option<Uuid>,
) -> Result<ResponsesHistoryContextLineageEntry> {
    let (actual_content_id, content_hash, _) = put_canonical_runtime_content_in_transaction(
        tx,
        flow.scope_id,
        flow.application_id,
        content,
    )
    .await?;
    let sequence=sqlx::query_scalar::<_,i64>("select coalesce(max(context_sequence),-1)+1 from runtime_context_projections where flow_run_id=$1").bind(flow.flow_run_id).fetch_one(&mut **tx).await?;
    let id = Uuid::now_v7();
    sqlx::query("insert into runtime_context_projections(id,flow_run_id,projection_kind,source_item_refs,model_input_ref,model_input_hash,provider_continuation_metadata,previous_projection_id,scope_id,application_id,context_sequence,transition_kind,transition_actor,actual_content_id) values($1,$2,'context_version','[]'::jsonb,'runtime_canonical_content:'||$3::text,$4,'{}'::jsonb,$5,$6,$7,$8,'append','host',$3)")
        .bind(id).bind(flow.flow_run_id).bind(actual_content_id).bind(&content_hash).bind(parent).bind(flow.scope_id).bind(flow.application_id).bind(sequence).execute(&mut **tx).await?;
    Ok(ResponsesHistoryContextLineageEntry {
        identity: ResponsesHistoryContextIdentity {
            context_version_id: id,
            actual_content_id,
            content_hash,
        },
        previous_projection_id: parent,
        sequence,
        content: content.clone(),
    })
}

pub(super) async fn copy_one(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    flow: &ResponsesHistoryFlowKey,
    change: &ResponsesHistoryContextChange,
    mapping: &mut HashMap<Uuid, ResponsesHistoryContextLineageEntry>,
) -> Result<ResponsesHistoryContextReceipt> {
    let old = &change.before;
    anyhow::ensure!(
        !mapping.contains_key(&old.identity.context_version_id),
        "responses_history_duplicate_context"
    );
    let parent = old.previous_projection_id.map(|parent| {
        mapping
            .get(&parent)
            .map_or(parent, |entry| entry.identity.context_version_id)
    });
    let after = append(tx, flow, &change.after_content, parent).await?;
    let receipt = ResponsesHistoryContextReceipt {
        before: old.identity.clone(),
        before_previous_projection_id: old.previous_projection_id,
        before_sequence: old.sequence,
        after: after.identity.clone(),
        after_previous_projection_id: after.previous_projection_id,
        after_sequence: after.sequence,
    };
    mapping.insert(old.identity.context_version_id, after);
    Ok(receipt)
}

/// Recognize a complete COW result through actual checkpoint/head roots. A shared
/// ancestor is matched once; no independent recursive per-checkpoint traversal.
pub(super) async fn match_after(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    flow: &ResponsesHistoryFlowKey,
    changes: &[ResponsesHistoryContextChange],
    seeds: Vec<(Uuid, Uuid)>,
    minimum_sequence: Option<i64>,
) -> Result<bool> {
    let planned: HashMap<_, _> = changes
        .iter()
        .map(|c| (c.before.identity.context_version_id, c))
        .collect();
    let mut mapped = HashMap::new();
    let mut used_new = HashSet::new();
    let mut pending = seeds;
    while let Some((old, new)) = pending.pop() {
        let Some(change) = planned.get(&old) else {
            if old != new {
                return Ok(false);
            }
            continue;
        };
        if let Some(previous) = mapped.get(&old) {
            if *previous != new {
                return Ok(false);
            }
            continue;
        }
        if old == new || !used_new.insert(new) {
            return Ok(false);
        }
        let actual = context(tx, flow, new).await?;
        if actual.entry.content != change.after_content
            || actual.transition_kind.as_deref() != Some("append")
            || actual.transition_actor.as_deref() != Some("host")
            || minimum_sequence.is_some_and(|minimum| actual.entry.sequence <= minimum)
        {
            return Ok(false);
        }
        match (
            change.before.previous_projection_id,
            actual.entry.previous_projection_id,
        ) {
            (Some(old_parent), Some(new_parent)) => pending.push((old_parent, new_parent)),
            (None, None) => {}
            _ => return Ok(false),
        }
        mapped.insert(old, new);
    }
    Ok(mapped.len() == changes.len())
}

pub(super) async fn verify_receipts(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    flow: &ResponsesHistoryFlowKey,
    receipts: &[ResponsesHistoryContextReceipt],
) -> Result<()> {
    let mut old_ids = HashSet::new();
    let mut new_ids = HashSet::new();
    for receipt in receipts {
        anyhow::ensure!(
            old_ids.insert(receipt.before.context_version_id)
                && new_ids.insert(receipt.after.context_version_id),
            "responses_history_duplicate_context"
        );
        let before = context(tx, flow, receipt.before.context_version_id)
            .await?
            .entry;
        let after = context(tx, flow, receipt.after.context_version_id)
            .await?
            .entry;
        anyhow::ensure!(
            before.identity == receipt.before
                && before.previous_projection_id == receipt.before_previous_projection_id
                && before.sequence == receipt.before_sequence
                && after.identity == receipt.after
                && after.previous_projection_id == receipt.after_previous_projection_id
                && after.sequence == receipt.after_sequence,
            "responses_history_context_receipt_conflict"
        );
    }
    Ok(())
}

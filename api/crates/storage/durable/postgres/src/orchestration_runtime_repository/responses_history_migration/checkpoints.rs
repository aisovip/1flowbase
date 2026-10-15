use super::*;

const MARKER: &str = "__runtime_recovery_context";

pub(super) struct ContextRecord {
    pub entry: ResponsesHistoryContextLineageEntry,
    pub transition_kind: Option<String>,
    pub transition_actor: Option<String>,
}

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

pub(super) fn replacement_content(change: &ResponsesHistoryCheckpointChange) -> Result<Value> {
    let mut pool = change
        .replacement_variable_pool
        .as_object()
        .cloned()
        .ok_or_else(|| anyhow!("responses_history_checkpoint_pool_invalid"))?;
    pool.remove(MARKER);
    Ok(json!({"format":"runtime_snapshot_v1","variable_pool":pool}))
}

fn projected_checkpoint(
    change: &ResponsesHistoryCheckpointChange,
    id: Uuid,
    sequence: i64,
) -> Result<(Value, Value)> {
    let mut locator = change.before_locator_payload.clone();
    anyhow::ensure!(
        locator.is_object(),
        "responses_history_checkpoint_locator_invalid"
    );
    locator["context_version_id"] = json!(id);
    anyhow::ensure!(
        change.before_variable_snapshot.is_object(),
        "responses_history_checkpoint_snapshot_invalid"
    );
    let snapshot = json!({MARKER:{"context_version_id":id,"sequence":sequence,"parent_context_version_id":null}});
    Ok((locator, snapshot))
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

pub(super) async fn lineage(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    flow: &ResponsesHistoryFlowKey,
    locator: &Value,
) -> Result<Vec<ResponsesHistoryContextLineageEntry>> {
    let mut next = locator_context(locator)?;
    let mut visited = std::collections::HashSet::new();
    let mut result: Vec<ResponsesHistoryContextLineageEntry> = Vec::new();
    while let Some(id) = next {
        anyhow::ensure!(
            visited.insert(id),
            "responses_history_checkpoint_lineage_cycle"
        );
        let record = context(tx, flow, id).await?;
        if let Some(child) = result.last() {
            anyhow::ensure!(
                record.entry.sequence < child.sequence,
                "responses_history_checkpoint_sequence_invalid"
            );
        }
        next = record.entry.previous_projection_id;
        result.push(record.entry);
    }
    result.reverse();
    Ok(result)
}

pub(super) async fn load_all(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    flow: &ResponsesHistoryFlowKey,
    lock: bool,
) -> Result<Vec<ResponsesHistoryCheckpoint>> {
    let suffix = if lock { "for update of c" } else { "" };
    let rows=sqlx::query(&format!("select c.id,c.scope_id,c.node_run_id,(n.flow_run_id=$1 and n.scope_id=$2) as node_valid,runtime_original_json(c.locator_payload,c.raw_json_payloads,'locator_payload') as locator_payload,runtime_original_json(c.variable_snapshot,c.raw_json_payloads,'variable_snapshot') as variable_snapshot from flow_run_checkpoints c left join node_runs n on n.id=c.node_run_id where c.flow_run_id=$1 order by c.id {suffix}"))
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
        let lineage = lineage(tx, flow, &locator_payload).await?;
        result.push(ResponsesHistoryCheckpoint {
            checkpoint_id: id,
            locator_payload,
            variable_snapshot: row.try_get("variable_snapshot")?,
            lineage,
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

pub(super) async fn verify_before(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    flow: &ResponsesHistoryFlowKey,
    change: &ResponsesHistoryCheckpointChange,
) -> Result<()> {
    let locator_id = locator_context(&change.before_locator_payload)?;
    anyhow::ensure!(
        locator_id
            == change
                .before_context
                .as_ref()
                .map(|id| id.context_version_id),
        "responses_history_checkpoint_preimage_context_invalid"
    );
    if let Some(identity) = &change.before_context {
        anyhow::ensure!(
            context(tx, flow, identity.context_version_id)
                .await?
                .entry
                .identity
                == *identity,
            "responses_history_checkpoint_preimage_context_changed"
        );
    }
    replacement_content(change)?;
    Ok(())
}

pub(super) async fn is_after(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    flow: &ResponsesHistoryFlowKey,
    current: &ResponsesHistoryCheckpoint,
    change: &ResponsesHistoryCheckpointChange,
) -> Result<bool> {
    let Some(id) = locator_context(&current.locator_payload)? else {
        return Ok(false);
    };
    if Some(id) == change.before_context.as_ref().map(|c| c.context_version_id) {
        return Ok(false);
    }
    let record = context(tx, flow, id).await?;
    let (locator, snapshot) = projected_checkpoint(change, id, record.entry.sequence)?;
    Ok(record.entry.content == replacement_content(change)?
        && record.entry.previous_projection_id.is_none()
        && record.transition_kind.as_deref() == Some("append")
        && record.transition_actor.as_deref() == Some("host")
        && current.locator_payload == locator
        && current.variable_snapshot == snapshot)
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

pub(super) async fn apply(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    flow: &ResponsesHistoryFlowKey,
    changes: &[ResponsesHistoryCheckpointChange],
) -> Result<Vec<ResponsesHistoryCheckpointReceipt>> {
    let mut result = Vec::with_capacity(changes.len());
    for change in changes {
        let before_shadow = read_shadow(tx, flow, change.checkpoint_id).await?;
        let content = replacement_content(change)?;
        let (content_id, hash, _) = put_canonical_runtime_content_in_transaction(
            tx,
            flow.scope_id,
            flow.application_id,
            &content,
        )
        .await?;
        let sequence=sqlx::query_scalar::<_,i64>("select coalesce(max(context_sequence),-1)+1 from runtime_context_projections where flow_run_id=$1").bind(flow.flow_run_id).fetch_one(&mut **tx).await?;
        let id = Uuid::now_v7();
        sqlx::query("insert into runtime_context_projections(id,flow_run_id,projection_kind,source_item_refs,model_input_ref,model_input_hash,provider_continuation_metadata,previous_projection_id,scope_id,application_id,context_sequence,transition_kind,transition_actor,actual_content_id) values($1,$2,'context_version','[]'::jsonb,'runtime_canonical_content:'||$3::text,$4,'{}'::jsonb,null,$5,$6,$7,'append','host',$3)")
            .bind(id).bind(flow.flow_run_id).bind(content_id).bind(&hash).bind(flow.scope_id).bind(flow.application_id).bind(sequence).execute(&mut **tx).await?;
        let (after_locator_payload, after_variable_snapshot) =
            projected_checkpoint(change, id, sequence)?;
        let after_context = ResponsesHistoryContextIdentity {
            context_version_id: id,
            actual_content_id: content_id,
            content_hash: hash.clone(),
        };
        let mut after_shadow = before_shadow.clone();
        if let Some(binding) = &mut after_shadow {
            // The legacy shadow owns the exact mutable source projection. The
            // full recovery context has a separate immutable content owner.
            let (marker_id, marker_hash, marker_size) =
                put_canonical_runtime_content_in_transaction(
                    tx,
                    flow.scope_id,
                    flow.application_id,
                    &after_variable_snapshot,
                )
                .await?;
            binding.source_hash = marker_hash;
            binding.source_byte_size = marker_size;
            binding.canonical_content_id = marker_id;
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
            after_context,
            after_context_sequence: sequence,
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
    for receipt in receipts {
        let checkpoint = current
            .iter()
            .find(|c| c.checkpoint_id == receipt.checkpoint_id)
            .ok_or_else(|| anyhow!("responses_history_checkpoint_missing"))?;
        anyhow::ensure!(
            checkpoint.locator_payload == receipt.after_locator_payload
                && checkpoint.variable_snapshot == receipt.after_variable_snapshot
                && checkpoint
                    .lineage
                    .last()
                    .is_some_and(|last| last.identity == receipt.after_context
                        && last.sequence == receipt.after_context_sequence),
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
        if let Some(identity) = &receipt.before_context {
            anyhow::ensure!(
                context(tx, flow, identity.context_version_id)
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

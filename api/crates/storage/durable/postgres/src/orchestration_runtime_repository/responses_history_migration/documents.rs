use super::*;

/// SQL identifiers come exclusively from this finite adapter-owned mapping.
fn location(
    owner: ResponsesHistoryDocumentOwner,
    flow: &ResponsesHistoryFlowKey,
) -> (&'static str, &'static str, &'static str, Uuid) {
    match owner {
        ResponsesHistoryDocumentOwner::FlowInput => {
            ("flow_runs", "flow_runs", "input_payload", flow.flow_run_id)
        }
        ResponsesHistoryDocumentOwner::FlowOutput => {
            ("flow_runs", "flow_runs", "output_payload", flow.flow_run_id)
        }
        ResponsesHistoryDocumentOwner::CallbackRequest { callback_task_id } => (
            "flow_run_callback_tasks",
            "flow_run_callback_tasks",
            "request_payload",
            callback_task_id,
        ),
        ResponsesHistoryDocumentOwner::NodeOutput { node_run_id } => (
            "node_run_records",
            "node_runs",
            "output_payload",
            node_run_id,
        ),
        ResponsesHistoryDocumentOwner::SpanMetadata { span_id } => {
            ("runtime_spans", "runtime_spans", "metadata", span_id)
        }
    }
}

pub(super) async fn read_current(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    flow: &ResponsesHistoryFlowKey,
    owner: ResponsesHistoryDocumentOwner,
    lock: bool,
) -> Result<Value> {
    let (table, lock_table, column, id) = location(owner, flow);
    // Share the converter's source lock, including when no shadow exists yet.
    // This prevents a legacy conversion from creating a stale shadow mid-write.
    if lock {
        sqlx::query("select pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!("{table}:{column}:{id}"))
            .execute(&mut **tx)
            .await?;
    }
    let flow_predicate = if lock_table == "flow_runs" {
        "r.id=f.id"
    } else {
        "r.flow_run_id=f.id"
    };
    let node_predicate = match owner {
        ResponsesHistoryDocumentOwner::CallbackRequest { .. } | ResponsesHistoryDocumentOwner::SpanMetadata { .. } =>
            "and (r.node_run_id is null or exists(select 1 from node_runs n where n.id=r.node_run_id and n.flow_run_id=f.id and n.scope_id=f.scope_id))",
        _ => "",
    };
    let lock_suffix = if lock { "for update of r" } else { "" };
    // node_run_records is an outer-join view: lock its physical owner first.
    let locked = sqlx::query_scalar::<_, Uuid>(&format!(
        "select r.id from {lock_table} r join flow_runs f on {flow_predicate} where r.id=$1 and f.id=$2 and f.scope_id=$3 and f.application_id=$4 and r.scope_id=$3 {node_predicate} {lock_suffix}"
    ))
    .bind(id).bind(flow.flow_run_id).bind(flow.scope_id).bind(flow.application_id)
    .fetch_optional(&mut **tx).await?;
    anyhow::ensure!(
        locked.is_some(),
        "responses_history_document_owner_mismatch"
    );
    Ok(sqlx::query_scalar::<_, Value>(&format!(
        "select runtime_original_json({column},raw_json_payloads,'{column}') from {table} where id=$1"
    )).bind(id).fetch_one(&mut **tx).await?)
}

pub(super) async fn read_shadow(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    flow: &ResponsesHistoryFlowKey,
    owner: ResponsesHistoryDocumentOwner,
) -> Result<Option<ResponsesHistoryShadowBinding>> {
    let (table, _, column, id) = location(owner, flow);
    let row = sqlx::query(
        "select id,scope_id,application_id,flow_run_id,source_hash,source_byte_size,canonical_content_id,context_version_id from runtime_legacy_shadow_rows where source_table=$1 and source_column=$2 and source_row_id=$3 for update",
    ).bind(table).bind(column).bind(id).fetch_optional(&mut **tx).await?;
    row.map(|row| {
        anyhow::ensure!(
            row.try_get::<Uuid, _>("scope_id")? == flow.scope_id
                && row.try_get::<Uuid, _>("application_id")? == flow.application_id
                && row.try_get::<Uuid, _>("flow_run_id")? == flow.flow_run_id,
            "responses_history_shadow_owner_mismatch"
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

pub(super) async fn verify_shadow_content(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    flow: &ResponsesHistoryFlowKey,
    binding: &ResponsesHistoryShadowBinding,
    value: &Value,
) -> Result<()> {
    let (hash, size) = canonical_runtime_json_identity(value)?;
    anyhow::ensure!(
        hash == binding.source_hash && size == binding.source_byte_size,
        "responses_history_stale_shadow"
    );
    let content = sqlx::query_as::<_, (Value,String,i64)>(
        "select runtime_original_json(content,raw_json_payloads,'content'),content_hash,byte_size from runtime_canonical_contents where id=$1 and scope_id=$2 and application_id=$3",
    ).bind(binding.canonical_content_id).bind(flow.scope_id).bind(flow.application_id)
    .fetch_optional(&mut **tx).await?
    .ok_or_else(|| anyhow!("responses_history_shadow_content_missing"))?;
    anyhow::ensure!(
        content == (value.clone(), hash, size),
        "responses_history_stale_shadow"
    );
    Ok(())
}

async fn write_document(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    flow: &ResponsesHistoryFlowKey,
    owner: ResponsesHistoryDocumentOwner,
    value: &Value,
) -> Result<()> {
    let (table, _, column, id) = location(owner, flow);
    let updated = sqlx::query(&format!(
        "update {table} set {column}=($2::jsonb->0),raw_json_payloads=(raw_json_payloads-'{column}') || jsonb_strip_nulls(jsonb_build_object('{column}',($2::jsonb->1))) where id=$1"
    )).bind(id).bind(lossless_json_parameter(value)).execute(&mut **tx).await?;
    anyhow::ensure!(
        updated.rows_affected() == 1,
        "responses_history_document_write_conflict"
    );
    Ok(())
}

pub(super) async fn write_shadow(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    binding: &ResponsesHistoryShadowBinding,
) -> Result<()> {
    let updated = sqlx::query(
        "update runtime_legacy_shadow_rows set source_hash=$2,source_byte_size=$3,canonical_content_id=$4,context_version_id=$5 where id=$1",
    ).bind(binding.shadow_row_id).bind(&binding.source_hash).bind(binding.source_byte_size)
    .bind(binding.canonical_content_id).bind(binding.context_version_id)
    .execute(&mut **tx).await?;
    anyhow::ensure!(
        updated.rows_affected() == 1,
        "responses_history_shadow_write_conflict"
    );
    Ok(())
}

/// Caller owns the transaction and must call lock_flow at its entrance. This
/// participant never commits and never interprets or rewrites proof fields.
pub(super) async fn apply(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    flow: &ResponsesHistoryFlowKey,
    changes: &[ResponsesHistoryDocumentChange],
) -> Result<Vec<ResponsesHistoryDocumentReceipt>> {
    let mut seen = std::collections::HashSet::new();
    let mut receipts = Vec::with_capacity(changes.len());
    // Validate every document and shadow before the first write.
    for change in changes {
        anyhow::ensure!(
            seen.insert(change.owner),
            "responses_history_duplicate_document"
        );
        anyhow::ensure!(
            read_current(tx, flow, change.owner, true).await? == change.before,
            "responses_history_document_cas_conflict"
        );
        let before_shadow = read_shadow(tx, flow, change.owner).await?;
        if let Some(binding) = &before_shadow {
            verify_shadow_content(tx, flow, binding, &change.before).await?;
        }
        receipts.push(ResponsesHistoryDocumentReceipt {
            change: change.clone(),
            after_shadow: before_shadow.clone(),
            before_shadow,
        });
    }
    for receipt in &mut receipts {
        if receipt.change.before == receipt.change.after {
            continue;
        }
        if let Some(binding) = &mut receipt.after_shadow {
            let (content_id, hash, size) = put_canonical_runtime_content_in_transaction(
                tx,
                flow.scope_id,
                flow.application_id,
                &receipt.change.after,
            )
            .await?;
            binding.canonical_content_id = content_id;
            binding.source_hash = hash;
            binding.source_byte_size = size;
            write_shadow(tx, binding).await?;
        }
        write_document(tx, flow, receipt.change.owner, &receipt.change.after).await?;
    }
    Ok(receipts)
}

/// Rollback CAS checks both mutable source and shadow; immutable content remains.
pub(super) async fn rollback(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    flow: &ResponsesHistoryFlowKey,
    receipts: &[ResponsesHistoryDocumentReceipt],
) -> Result<()> {
    let mut seen = std::collections::HashSet::new();
    for receipt in receipts {
        anyhow::ensure!(
            seen.insert(receipt.change.owner),
            "responses_history_duplicate_document"
        );
        anyhow::ensure!(
            read_current(tx, flow, receipt.change.owner, true).await? == receipt.change.after,
            "responses_history_document_rollback_conflict"
        );
        let shadow = read_shadow(tx, flow, receipt.change.owner).await?;
        anyhow::ensure!(
            shadow == receipt.after_shadow,
            "responses_history_shadow_rollback_conflict"
        );
        anyhow::ensure!(
            receipt.before_shadow.is_some() == receipt.after_shadow.is_some(),
            "responses_history_receipt_shadow_invalid"
        );
        if let (Some(before), Some(after)) = (&receipt.before_shadow, &receipt.after_shadow) {
            anyhow::ensure!(
                before.shadow_row_id == after.shadow_row_id,
                "responses_history_receipt_shadow_invalid"
            );
            verify_shadow_content(tx, flow, before, &receipt.change.before).await?;
            verify_shadow_content(tx, flow, after, &receipt.change.after).await?;
        }
    }
    for receipt in receipts {
        write_document(tx, flow, receipt.change.owner, &receipt.change.before).await?;
        if let Some(binding) = &receipt.before_shadow {
            write_shadow(tx, binding).await?;
        }
    }
    Ok(())
}

/// Enumerate every finite owner in this flow. A corrupt source owner fails closed.
pub(super) async fn load_all(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    flow: &ResponsesHistoryFlowKey,
) -> Result<Vec<ResponsesHistoryDocument>> {
    let mut owners = vec![
        ResponsesHistoryDocumentOwner::FlowInput,
        ResponsesHistoryDocumentOwner::FlowOutput,
    ];
    for id in sqlx::query_scalar::<_, Uuid>(
        "select id from flow_run_callback_tasks where flow_run_id=$1 order by id",
    )
    .bind(flow.flow_run_id)
    .fetch_all(&mut **tx)
    .await?
    {
        owners.push(ResponsesHistoryDocumentOwner::CallbackRequest {
            callback_task_id: id,
        });
    }
    for id in
        sqlx::query_scalar::<_, Uuid>("select id from node_runs where flow_run_id=$1 order by id")
            .bind(flow.flow_run_id)
            .fetch_all(&mut **tx)
            .await?
    {
        owners.push(ResponsesHistoryDocumentOwner::NodeOutput { node_run_id: id });
    }
    for id in sqlx::query_scalar::<_, Uuid>(
        "select id from runtime_spans where flow_run_id=$1 order by id",
    )
    .bind(flow.flow_run_id)
    .fetch_all(&mut **tx)
    .await?
    {
        owners.push(ResponsesHistoryDocumentOwner::SpanMetadata { span_id: id });
    }
    let mut result = Vec::with_capacity(owners.len());
    for owner in owners {
        result.push(ResponsesHistoryDocument {
            owner,
            value: read_current(tx, flow, owner, false).await?,
        });
    }
    Ok(result)
}

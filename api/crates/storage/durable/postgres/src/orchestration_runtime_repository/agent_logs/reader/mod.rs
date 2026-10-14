//! Selected immutable source facts, independent of native flow-run archives.
use super::super::*;
use control_plane_contracts::ports::{AgentLogEvent, AgentLogsBatch, AGENT_LOGS_SCHEMA_VERSION};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) async fn read(
    store: &PgControlPlaneStore,
    application_id: Uuid,
    scope_id: Uuid,
    record_ids: &[Uuid],
) -> Result<Vec<AgentLogsBatch>> {
    anyhow::ensure!(!record_ids.is_empty(), "agent_logs.read_selection_empty");
    let selected: Vec<Uuid> = record_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut tx = store.pool().begin().await?;
    // Validation and content reads see one snapshot even if ingestion/deletion commits.
    sqlx::query("set transaction isolation level repeatable read, read only")
        .execute(&mut *tx)
        .await?;
    let present: i64 = sqlx::query_scalar(
        "select count(*) from application_run_log_tasks t join applications a on a.id=t.application_id and a.scope_id=t.scope_id where t.application_id=$1 and t.scope_id=$2 and t.id=any($3) and t.source_kind='imported' and a.application_type='agent_logs'",
    ).bind(application_id).bind(scope_id).bind(&selected).fetch_one(&mut *tx).await?;
    anyhow::ensure!(
        present as usize == selected.len(),
        "agent_logs.read_selection_invalid"
    );
    // LEFT joins make a missing receipt/body an integrity error rather than silently
    // dropping a selected record or one of its events. Original JSON restores raw facts.
    let rows = sqlx::query(
        r#"select t.id as record_id,t.source_id,t.source_client,
        t.source_session_id,t.source_task_id,r.event_id,
        runtime_original_json(c.content,c.raw_json_payloads,'content') as body
        from application_run_log_tasks t
        left join application_log_upload_receipts r
          on r.application_id=t.application_id and r.record_id=t.id and r.source_id=t.source_id
        left join client_trajectory_sections s
          on s.step_id=r.step_id and s.record_id=t.id and s.section='overview'
        left join runtime_canonical_contents c
          on c.id=s.content_id and c.application_id=t.application_id and c.scope_id=t.scope_id
        where t.application_id=$1 and t.scope_id=$2 and t.id=any($3)
        order by t.source_id,t.source_client,s.event_sequence,r.event_id"#,
    )
    .bind(application_id)
    .bind(scope_id)
    .bind(&selected)
    .fetch_all(&mut *tx)
    .await?;
    let mut batches = BTreeMap::<(String, String), AgentLogsBatch>::new();
    let mut loaded = BTreeSet::new();
    for row in rows {
        let source_id: Option<String> = row.try_get("source_id")?;
        let source_client: Option<String> = row.try_get("source_client")?;
        let body: Option<Value> = row.try_get("body")?;
        let (Some(source_id), Some(source_client), Some(body)) = (source_id, source_client, body)
        else {
            anyhow::bail!("agent_logs.read_fact_missing");
        };
        let event: AgentLogEvent = serde_json::from_value(body)?;
        anyhow::ensure!(
            row.try_get::<Option<String>, _>("event_id")?.as_deref()
                == Some(event.event_id.as_str())
                && row
                    .try_get::<Option<String>, _>("source_session_id")?
                    .as_deref()
                    == Some(event.source_session_id.as_str())
                && row
                    .try_get::<Option<String>, _>("source_task_id")?
                    .as_deref()
                    == Some(event.source_task_id.as_str()),
            "agent_logs.read_fact_identity"
        );
        loaded.insert(row.try_get::<Uuid, _>("record_id")?);
        batches
            .entry((source_id.clone(), source_client.clone()))
            .or_insert_with(|| AgentLogsBatch {
                schema_version: AGENT_LOGS_SCHEMA_VERSION.into(),
                source_id,
                source_client,
                events: vec![],
            })
            .events
            .push(event);
    }
    anyhow::ensure!(
        loaded.len() == selected.len(),
        "agent_logs.read_fact_missing"
    );
    let batches: Vec<_> = batches.into_values().collect();
    for batch in &batches {
        batch.validate()?;
    }
    tx.commit().await?;
    Ok(batches)
}

#[cfg(test)]
#[path = "_tests/selected_facts.rs"]
mod selected_facts;

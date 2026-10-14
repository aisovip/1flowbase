//! Selected immutable source facts, independent of native flow-run archives.
use super::super::*;
use control_plane_contracts::ports::{
    AgentLogEvent, AgentLogsBatch, AgentLogsExportFacts, AgentLogsExportSelector,
    AGENT_LOGS_SCHEMA_VERSION,
};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) async fn read(
    store: &PgControlPlaneStore,
    application_id: Uuid,
    scope_id: Uuid,
    record_ids: &[Uuid],
) -> Result<Vec<AgentLogsBatch>> {
    Ok(read_selection(
        store,
        application_id,
        scope_id,
        &AgentLogsExportSelector::Records(record_ids.to_vec()),
    )
    .await?
    .batches)
}

pub(crate) async fn read_selection(
    store: &PgControlPlaneStore,
    application_id: Uuid,
    scope_id: Uuid,
    selector: &AgentLogsExportSelector,
) -> Result<AgentLogsExportFacts> {
    let mut tx = store.pool().begin().await?;
    // Validation and content reads see one snapshot even if ingestion/deletion commits.
    sqlx::query("set transaction isolation level repeatable read, read only")
        .execute(&mut *tx)
        .await?;
    let facts = read_snapshot(&mut tx, application_id, scope_id, selector).await?;
    tx.commit().await?;
    Ok(facts)
}

async fn read_snapshot(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    application_id: Uuid,
    scope_id: Uuid,
    selector: &AgentLogsExportSelector,
) -> Result<AgentLogsExportFacts> {
    let ids = match selector {
        AgentLogsExportSelector::Records(ids) | AgentLogsExportSelector::Conversations(ids) => ids,
    };
    anyhow::ensure!(!ids.is_empty(), "agent_logs.read_selection_empty");
    let requested: Vec<Uuid> = ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let selected = match selector {
        AgentLogsExportSelector::Records(_) => requested,
        AgentLogsExportSelector::Conversations(_) => {
            // Use the canonical conversation FK, including child tasks. Do not
            // infer membership from the latest record locator or session strings.
            let present: i64 = sqlx::query_scalar(
                "select count(*) from application_conversations c join applications a on a.id=c.application_id and a.scope_id=c.scope_id where c.application_id=$1 and c.scope_id=$2 and c.id=any($3) and c.source_kind='imported' and a.application_type='agent_logs' and exists(select 1 from application_run_log_tasks t where t.log_conversation_id=c.id) and not exists(select 1 from application_run_log_tasks t where t.log_conversation_id=c.id and (t.application_id<>c.application_id or t.scope_id<>c.scope_id or t.source_kind<>'imported'))",
            ).bind(application_id).bind(scope_id).bind(&requested).fetch_one(&mut **tx).await?;
            anyhow::ensure!(
                present as usize == requested.len(),
                "agent_logs.read_selection_invalid"
            );
            sqlx::query_scalar::<_, Uuid>(
                "select t.id from application_run_log_tasks t where t.application_id=$1 and t.scope_id=$2 and t.log_conversation_id=any($3) and t.source_kind='imported' order by t.id",
            ).bind(application_id).bind(scope_id).bind(&requested).fetch_all(&mut **tx).await?
        }
    };
    let present: i64 = sqlx::query_scalar(
        "select count(*) from application_run_log_tasks t join applications a on a.id=t.application_id and a.scope_id=t.scope_id where t.application_id=$1 and t.scope_id=$2 and t.id=any($3) and t.source_kind='imported' and a.application_type='agent_logs'",
    ).bind(application_id).bind(scope_id).bind(&selected).fetch_one(&mut **tx).await?;
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
    .fetch_all(&mut **tx)
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
    Ok(AgentLogsExportFacts {
        batches,
        record_ids: selected,
    })
}

#[cfg(test)]
#[path = "_tests/selected_facts.rs"]
mod selected_facts;

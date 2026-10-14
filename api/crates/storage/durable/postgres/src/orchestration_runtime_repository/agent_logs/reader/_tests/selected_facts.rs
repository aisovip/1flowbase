use super::*;
use control_plane_contracts::ports::{
    AgentLogEventKind, AgentLogUsage, AgentLogUsageBasis, OrchestrationRuntimeRepository,
};

async fn setup() -> (PgControlPlaneStore, Uuid, Uuid, Uuid) {
    let (pool, native_run) =
        crate::_tests::provider_protocol_capsule_store_tests::seeded_flow_run().await;
    let store = PgControlPlaneStore::new(pool);
    let (application_id, scope_id): (Uuid, Uuid) =
        sqlx::query_as("select application_id,scope_id from flow_runs where id=$1")
            .bind(native_run)
            .fetch_one(store.pool())
            .await
            .unwrap();
    // Keep a real native task to prove that a mixed selection cannot export it.
    sqlx::query("insert into application_run_log_summaries(flow_run_id,application_id,run_mode,status,title,input_payload,started_at,created_at,updated_at,scope_id) select id,application_id,run_mode,status,title,input_payload,started_at,created_at,updated_at,scope_id from flow_runs where id=$1")
        .bind(native_run).execute(store.pool()).await.unwrap();
    sqlx::query("select application_run_log_task_refresh($1)")
        .bind(native_run)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query(
        "update applications set application_type='agent_logs',api_enabled=true where id=$1",
    )
    .bind(application_id)
    .execute(store.pool())
    .await
    .unwrap();
    (store, application_id, scope_id, native_run)
}

fn event(id: &str, session: &str, task: &str, sequence: i64) -> AgentLogEvent {
    AgentLogEvent {
        event_id: id.into(),
        source_session_id: session.into(),
        source_task_id: task.into(),
        parent_source_task_id: Some("parent-task".into()),
        sequence,
        occurred_at: "2026-10-07T08:00:00.123Z".into(),
        kind: AgentLogEventKind::Assistant,
        content: Some("original assistant text\nsecond line".into()),
        phase: Some("final_answer".into()),
        name: Some("source-name".into()),
        call_id: Some("call-original".into()),
        model_id: Some("unknown-model".into()),
        reasoning_effort: Some("high".into()),
        provider_code: Some("unknown-provider".into()),
        usage: Some(AgentLogUsage {
            basis: AgentLogUsageBasis::Cumulative,
            response_id: Some("response-original".into()),
            input_tokens: Some(13),
            output_tokens: Some(7),
            input_cache_hit_tokens: Some(2),
            cache_write_tokens: Some(3),
            total_tokens: Some(20),
        }),
        inherited: true,
        raw: json!({"original":id,"nul":"embedded\u{0}value","nested":{"number":987654321,"array":[true,null,"preserve"]}}),
    }
}

async fn ingest(
    store: &PgControlPlaneStore,
    app: Uuid,
    scope: Uuid,
    source: &str,
    events: Vec<AgentLogEvent>,
) -> Vec<Uuid> {
    let costs = vec![None; events.len()];
    store
        .ingest_agent_logs(
            app,
            scope,
            Some(Uuid::now_v7()),
            &AgentLogsBatch {
                schema_version: AGENT_LOGS_SCHEMA_VERSION.into(),
                source_id: source.into(),
                source_client: "codex".into(),
                events,
            },
            &costs,
        )
        .await
        .unwrap()
        .record_ids
}

#[tokio::test]
async fn imported_reader_preserves_selected_multi_session_facts_and_source_order() {
    let (store, app, scope, _) = setup().await;
    let later = event("z-later", "session-a", "task-a", 9);
    let first = event("a-first", "session-a", "task-a", 2);
    let tied = event("b-tied", "session-b", "task-b", 2);
    let records = ingest(
        &store,
        app,
        scope,
        "source-a",
        vec![later.clone(), tied.clone(), first.clone()],
    )
    .await;
    assert_eq!(records.len(), 2);
    let excluded = ingest(
        &store,
        app,
        scope,
        "source-a",
        vec![event("excluded", "session-c", "task-c", 0)],
    )
    .await[0];
    let other = event("a-first", "session-other", "task-other", 1);
    let other_id = ingest(&store, app, scope, "source-b", vec![other.clone()]).await[0];
    // Corrupt an unselected body's directory: the reader must not visit it.
    sqlx::query("delete from client_trajectory_sections where record_id=$1 and section='overview'")
        .bind(excluded)
        .execute(store.pool())
        .await
        .unwrap();
    let selection = [other_id, records[1], records[0], records[0]];
    let exported = store
        .read_imported_agent_logs(app, scope, &selection)
        .await
        .unwrap();
    assert_eq!(exported.len(), 2);
    assert_eq!(exported[0].source_id, "source-a");
    assert_eq!(exported[0].source_client, "codex");
    assert_eq!(exported[0].schema_version, AGENT_LOGS_SCHEMA_VERSION);
    assert_eq!(
        serde_json::to_value(&exported[0].events).unwrap(),
        serde_json::to_value([first, tied, later]).unwrap()
    );
    assert_eq!(exported[1].source_id, "source-b");
    assert_eq!(
        serde_json::to_value(&exported[1].events).unwrap(),
        serde_json::to_value([other]).unwrap()
    );
}

#[tokio::test]
async fn imported_reader_rejects_partial_missing_foreign_native_and_incomplete_selections() {
    let (store, app, scope, native) = setup().await;
    let id = ingest(
        &store,
        app,
        scope,
        "collector",
        vec![event("source-event", "session", "task", 1)],
    )
    .await[0];
    for (application_id, scope_id, selection) in [
        (app, scope, vec![id, Uuid::now_v7()]),
        (app, Uuid::now_v7(), vec![id]),
        (Uuid::now_v7(), scope, vec![id]),
        (app, scope, vec![id, native]),
    ] {
        let error = store
            .read_imported_agent_logs(application_id, scope_id, &selection)
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "agent_logs.read_selection_invalid");
    }
    assert_eq!(
        store
            .read_imported_agent_logs(app, scope, &[])
            .await
            .unwrap_err()
            .to_string(),
        "agent_logs.read_selection_empty"
    );
    // Missing one persisted fact is an integrity failure, never a partial batch.
    sqlx::query("delete from client_trajectory_sections where record_id=$1 and section='overview'")
        .bind(id)
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        store
            .read_imported_agent_logs(app, scope, &[id])
            .await
            .unwrap_err()
            .to_string(),
        "agent_logs.read_fact_missing"
    );
}

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

fn conversation_events() -> Vec<AgentLogEvent> {
    let mut events = Vec::new();
    for (task_index, (session, task, parent)) in [
        ("session-a", "task-a", None),
        ("session-a", "task-b", Some("task-a")),
        ("session-b", "task-c", None),
    ]
    .into_iter()
    .enumerate()
    {
        for offset in 0..7 {
            let mut fact = event(
                &format!("{task}-{offset}"),
                session,
                task,
                (task_index * 7 + offset) as i64,
            );
            fact.parent_source_task_id = parent.map(str::to_owned);
            events.push(fact);
        }
    }
    events
}

#[tokio::test]
async fn conversation_selector_reads_two_conversations_three_tasks_twenty_one_original_facts() {
    let (store, app, scope, native) = setup().await;
    let expected = conversation_events();
    let records = ingest(&store, app, scope, "conversation-source", expected.clone()).await;
    let conversations: Vec<Uuid> = sqlx::query_scalar("select id from application_conversations where application_id=$1 and scope_id=$2 and source_kind='imported' order by id")
        .bind(app).bind(scope).fetch_all(store.pool()).await.unwrap();
    assert_eq!(conversations.len(), 2);
    assert_eq!(records.len(), 3);
    sqlx::query("update applications set api_enabled=false where id=$1")
        .bind(app)
        .execute(store.pool())
        .await
        .unwrap();
    let all = store
        .read_imported_agent_logs_selection(
            app,
            scope,
            &AgentLogsExportSelector::Conversations(vec![
                conversations[0],
                conversations[1],
                conversations[0],
            ]),
        )
        .await
        .unwrap();
    assert_eq!(all.record_ids, records);
    assert_eq!(
        serde_json::to_value(&all.batches[0].events).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
    let record:Uuid = sqlx::query_scalar("select id from application_run_log_tasks where application_id=$1 and source_task_id='task-a'").bind(app).fetch_one(store.pool()).await.unwrap();
    let exact = store
        .read_imported_agent_logs_selection(
            app,
            scope,
            &AgentLogsExportSelector::Records(vec![record]),
        )
        .await
        .unwrap();
    assert_eq!(exact.record_ids, vec![record]);
    assert_eq!(
        serde_json::to_value(&exact.batches[0].events).unwrap(),
        serde_json::to_value(&expected[..7]).unwrap()
    );
    let legacy = store
        .read_imported_agent_logs(app, scope, &[record])
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(legacy).unwrap(),
        serde_json::to_value(exact.batches).unwrap()
    );
    let native_conversation = Uuid::now_v7();
    sqlx::query("insert into application_conversations(id,application_id,scope_id,external_conversation_id,source_kind) values($1,$2,$3,'native-fixture','native')")
        .bind(native_conversation).bind(app).bind(scope).execute(store.pool()).await.unwrap();
    sqlx::query("update application_run_log_tasks set log_conversation_id=$1 where id=$2")
        .bind(native_conversation)
        .bind(native)
        .execute(store.pool())
        .await
        .unwrap();
    for (application_id, scope_id, ids) in [
        (app, scope, vec![conversations[0], Uuid::now_v7()]),
        (Uuid::now_v7(), scope, conversations.clone()),
        (app, Uuid::now_v7(), conversations.clone()),
        (app, scope, vec![native_conversation]),
        (app, scope, vec![conversations[0], native_conversation]),
    ] {
        let error = store
            .read_imported_agent_logs_selection(
                application_id,
                scope_id,
                &AgentLogsExportSelector::Conversations(ids),
            )
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "agent_logs.read_selection_invalid");
    }
    for selector in [
        AgentLogsExportSelector::Conversations(vec![]),
        AgentLogsExportSelector::Records(vec![]),
    ] {
        assert_eq!(
            store
                .read_imported_agent_logs_selection(app, scope, &selector)
                .await
                .unwrap_err()
                .to_string(),
            "agent_logs.read_selection_empty"
        );
    }
    // A native member cannot be silently omitted from an otherwise imported
    // selection, nor may an incomplete task body become a partial conversation.
    sqlx::query("update application_run_log_tasks set log_conversation_id=$1 where id=$2")
        .bind(conversations[0])
        .bind(native)
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        store
            .read_imported_agent_logs_selection(
                app,
                scope,
                &AgentLogsExportSelector::Conversations(conversations.clone())
            )
            .await
            .unwrap_err()
            .to_string(),
        "agent_logs.read_selection_invalid"
    );
    sqlx::query("update application_run_log_tasks set log_conversation_id=$1 where id=$2")
        .bind(native_conversation)
        .bind(native)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("delete from client_trajectory_sections where record_id=$1 and section='overview'")
        .bind(record)
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        store
            .read_imported_agent_logs_selection(
                app,
                scope,
                &AgentLogsExportSelector::Conversations(conversations)
            )
            .await
            .unwrap_err()
            .to_string(),
        "agent_logs.read_fact_missing"
    );
}

#[tokio::test]
async fn conversation_membership_and_source_facts_use_the_same_repeatable_read_snapshot() {
    let (store, app, scope, _) = setup().await;
    let mut first = event("first", "snapshot-session", "first-task", 0);
    first.parent_source_task_id = None;
    let first_record = ingest(&store, app, scope, "snapshot-source", vec![first.clone()]).await[0];
    let conversation: Uuid =
        sqlx::query_scalar("select log_conversation_id from application_run_log_tasks where id=$1")
            .bind(first_record)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let mut tx = store.pool().begin().await.unwrap();
    sqlx::query("set transaction isolation level repeatable read, read only")
        .execute(&mut *tx)
        .await
        .unwrap();
    // Establish the snapshot before a concurrent committed membership/fact change.
    sqlx::query("select id from application_conversations where id=$1")
        .bind(conversation)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    let mut later = event("later", "snapshot-session", "later-task", 1);
    later.parent_source_task_id = None;
    ingest(&store, app, scope, "snapshot-source", vec![later.clone()]).await;
    let selector = AgentLogsExportSelector::Conversations(vec![conversation]);
    let frozen = read_snapshot(&mut tx, app, scope, &selector).await.unwrap();
    assert_eq!(frozen.record_ids, vec![first_record]);
    assert_eq!(
        serde_json::to_value(&frozen.batches[0].events).unwrap(),
        serde_json::to_value([first.clone()]).unwrap()
    );
    tx.commit().await.unwrap();
    let fresh = store
        .read_imported_agent_logs_selection(app, scope, &selector)
        .await
        .unwrap();
    assert_eq!(fresh.record_ids.len(), 2);
    assert_eq!(
        serde_json::to_value(&fresh.batches[0].events).unwrap(),
        serde_json::to_value([first, later]).unwrap()
    );
}

use super::*;

#[tokio::test]
async fn imported_title_uses_only_current_human_input_across_incremental_batches() {
    let (store, scope, app) = setup().await;
    let service = AgentLogsService::new(store.clone());
    let key = Uuid::now_v7();
    // The checkpoint can already have acknowledged session_meta. Do not rely on
    // an empty header being the first event assigned to a business turn.
    for (index, kind) in [
        AgentLogEventKind::Context,
        AgentLogEventKind::System,
        AgentLogEventKind::Assistant,
        AgentLogEventKind::User,
    ]
    .into_iter()
    .enumerate()
    {
        let mut first = event(&format!("first-{index}"), 1, kind, Some("injected context"));
        first.source_task_id = format!("turn-{index}");
        first.inherited = first.kind == AgentLogEventKind::User;
        let id = service
            .ingest(app, scope, key, batch(vec![first.clone()]))
            .await
            .unwrap()
            .record_ids[0];
        let initial: (String, Option<String>) =
            sqlx::query_as("select title,user_input from application_run_log_tasks where id=$1")
                .bind(id)
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(initial, (first.source_task_id.clone(), None));
        let overview = store
            .application_log_record(app, id)
            .await
            .unwrap()
            .unwrap();
        assert!(!overview.messages.iter().any(|m| m.role == "user"));

        let mut user = event(
            &format!("user-{index}"),
            2,
            AgentLogEventKind::User,
            Some("real question"),
        );
        user.source_task_id = first.source_task_id.clone();
        service
            .ingest(app, scope, key, batch(vec![user]))
            .await
            .unwrap();
        let mut tail = event(
            &format!("context-{index}"),
            3,
            AgentLogEventKind::Context,
            Some("later context"),
        );
        tail.source_task_id = first.source_task_id.clone();
        service
            .ingest(app, scope, key, batch(vec![tail]))
            .await
            .unwrap();
        let actual: (String, Option<String>) =
            sqlx::query_as("select title,user_input from application_run_log_tasks where id=$1")
                .bind(id)
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(
            actual,
            ("real question".into(), Some("real question".into()))
        );
        let overview = store
            .application_log_record(app, id)
            .await
            .unwrap()
            .unwrap();
        let users: Vec<_> = overview
            .messages
            .iter()
            .filter(|m| m.role == "user")
            .collect();
        assert_eq!(users.len(), 1);
        assert_eq!(users[0].content, "real question");
        let trace = store
            .record_client_trajectory_page(app, id, None, 10)
            .await
            .unwrap();
        assert_eq!(trace.items.len(), 3, "context facts must remain available");
    }
}

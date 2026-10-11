use super::*;

#[tokio::test]
async fn agent_logs_conversation_seven_turn_four_clients_cursor_isolation_and_locator() {
    let (store, scope, app) = setup().await;
    let service = AgentLogsService::new(store.clone());
    let mut clients = Vec::new();
    for client in ["codex", "claude_code", "dsh", "opencode"] {
        let mut ids = Vec::new();
        for turn in 0..7 {
            let mut events = Vec::new();
            for (sequence, kind, content) in [
                (1, AgentLogEventKind::System, "system"),
                (2, AgentLogEventKind::User, "question"),
                (3, AgentLogEventKind::Assistant, "progress"),
                (4, AgentLogEventKind::Assistant, "final"),
            ] {
                let mut e = event(
                    &format!("{client}-{turn}-{sequence}"),
                    sequence,
                    kind,
                    Some(content),
                );
                e.source_task_id = format!("{client}-turn-{turn}");
                e.occurred_at = format!("2026-10-07T08:00:{:02}Z", turn / 2);
                e.reasoning_effort = Some("high".into());
                if sequence == 4 {
                    e.phase = Some("final_answer".into());
                }
                events.push(e);
            }
            let mut input = batch(events);
            input.source_client = client.into();
            // Deliberately identical source/session labels across clients.
            // Existing ingestion identity forbids client changes on one source task.
            let id = service
                .ingest(app, scope, Uuid::now_v7(), input)
                .await
                .unwrap()
                .record_ids[0];
            ids.push(id);
        }
        // Adjacent pairs tie in time, all other groups progress chronologically.
        for pair in ids.chunks_mut(2) {
            pair.sort();
        }
        let initial = store
            .application_log_record_conversation_page(app, ids[5], None, None, 5)
            .await
            .unwrap();
        assert_eq!(
            initial
                .items
                .iter()
                .map(|i| i.record_id)
                .collect::<Vec<_>>(),
            ids[1..6]
        );
        assert!(initial.page.has_before && initial.page.has_after);
        assert_eq!(initial.page.newest_cursor, Some(ids[5]));
        for item in &initial.items {
            assert_eq!(item.source_client.as_deref(), Some(client));
            assert_eq!(item.reasoning_effort.as_deref(), Some("high"));
            assert_eq!(
                item.messages
                    .iter()
                    .map(|m| m.content.as_str())
                    .collect::<Vec<_>>(),
                vec!["system", "question", "final"]
            );
            let trace = store
                .record_client_trajectory_page(app, item.record_id, None, 20)
                .await
                .unwrap();
            assert_eq!(trace.items.len(), 4);
            assert!(trace.items.iter().all(|s| s.request_id == item.record_id));
        }
        let older = store
            .application_log_record_conversation_page(
                app,
                ids[5],
                initial.page.before_cursor,
                None,
                5,
            )
            .await
            .unwrap();
        assert_eq!(
            older.items.iter().map(|i| i.record_id).collect::<Vec<_>>(),
            ids[..1]
        );
        assert!(!older.page.has_before);
        let newer = store
            .application_log_record_conversation_page(
                app,
                ids[5],
                None,
                initial.page.after_cursor,
                5,
            )
            .await
            .unwrap();
        assert_eq!(
            newer.items.iter().map(|i| i.record_id).collect::<Vec<_>>(),
            ids[6..]
        );
        assert!(!newer.page.has_after);
        // Starting at an older anchor must catch every newer turn, even with
        // several bounded forward requests. The cursor is the visible edge.
        let anchored = store
            .application_log_record_conversation_page(app, ids[1], None, None, 5)
            .await
            .unwrap();
        assert_eq!(anchored.page.newest_cursor, Some(ids[1]));
        let mut cursor = anchored.page.newest_cursor;
        let mut caught_up = Vec::new();
        loop {
            let page = store
                .application_log_record_conversation_page(app, ids[1], None, cursor, 2)
                .await
                .unwrap();
            caught_up.extend(page.items.iter().map(|item| item.record_id));
            assert_eq!(
                page.page.newest_cursor,
                page.items.last().map(|item| item.record_id)
            );
            if !page.page.has_after {
                break;
            }
            assert!(!page.items.is_empty());
            cursor = page.page.newest_cursor;
        }
        assert_eq!(caught_up, ids[2..]);
        let empty = store
            .application_log_record_conversation_page(app, ids[1], None, Some(ids[6]), 2)
            .await
            .unwrap();
        assert!(empty.items.is_empty());
        assert_eq!(empty.page.newest_cursor, None);
        clients.push(ids);
    }
    assert!(store
        .application_log_record_conversation_page(app, clients[0][5], Some(clients[1][0]), None, 5)
        .await
        .is_err());
    let mut foreign = event("foreign-source", 1, AgentLogEventKind::User, Some("secret"));
    foreign.source_task_id = "turn-0".into();
    let mut input = batch(vec![foreign]);
    input.source_id = "another-collector".into();
    let foreign_id = service
        .ingest(app, scope, Uuid::now_v7(), input)
        .await
        .unwrap()
        .record_ids[0];
    let mut different_session = event(
        "different-session",
        1,
        AgentLogEventKind::User,
        Some("different session secret"),
    );
    different_session.source_session_id = "another-session".into();
    let different_session_id = service
        .ingest(app, scope, Uuid::now_v7(), batch(vec![different_session]))
        .await
        .unwrap()
        .record_ids[0];
    assert!(store
        .application_log_record_conversation_page(
            app,
            clients[0][5],
            Some(different_session_id),
            None,
            5
        )
        .await
        .is_err());
    assert!(store
        .application_log_record_conversation_page(app, clients[0][5], Some(foreign_id), None, 5)
        .await
        .is_err());
    assert!(store
        .application_log_record_conversation_page(Uuid::now_v7(), clients[0][5], None, None, 5)
        .await
        .is_err());
    assert!(store
        .application_log_record_conversation_page(
            app,
            clients[0][5],
            Some(clients[0][0]),
            Some(clients[0][1]),
            5
        )
        .await
        .is_err());
    sqlx::query("update application_run_log_tasks set source_session_id=null where id=$1")
        .bind(foreign_id)
        .execute(store.pool())
        .await
        .unwrap();
    let single = store
        .application_log_record_conversation_page(app, foreign_id, None, None, 5)
        .await
        .unwrap();
    assert_eq!(single.items.len(), 1);
    assert!(!single.page.has_before && !single.page.has_after);
}

#[tokio::test]
async fn agent_logs_record_conversation_native_uses_existing_business_reader() {
    let db = isolated_database().await;
    let store = PgControlPlaneStore::new(db.connect().await.unwrap());
    run_migrations(store.pool()).await.unwrap();
    let seeded = seed_runtime_base(&store).await;
    let compiled = seed_compiled_plan(&store, &seeded).await;
    let run = seed_flow_run_with_mode(
        &store,
        &seeded,
        &compiled,
        OffsetDateTime::now_utc(),
        FlowRunMode::PublishedApiRun,
        None,
    )
    .await;
    let overview = store
        .application_log_record(seeded.application_id, run.id)
        .await
        .unwrap()
        .unwrap();
    let page = store
        .application_log_record_conversation_page(seeded.application_id, run.id, None, None, 5)
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(
        serde_json::to_value(&page.items[0]).unwrap(),
        serde_json::to_value(&overview).unwrap()
    );
    assert_eq!(page.items[0].native_run_id, Some(run.id));
}

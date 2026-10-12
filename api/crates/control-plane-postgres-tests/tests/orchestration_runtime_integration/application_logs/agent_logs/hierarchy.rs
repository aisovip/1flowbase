use super::*;

fn declared(task: &str, session: &str, parent: Option<&str>) -> AgentLogEvent {
    let mut e = event(
        &format!("{session}-{task}"),
        1,
        AgentLogEventKind::User,
        Some(task),
    );
    e.source_task_id = task.into();
    e.source_session_id = session.into();
    e.parent_source_task_id = parent.map(Into::into);
    e
}

async fn upload(store: &PgControlPlaneStore, scope: Uuid, app: Uuid, e: AgentLogEvent) -> Uuid {
    AgentLogsService::new(store.clone())
        .ingest(app, scope, Uuid::now_v7(), batch(vec![e]))
        .await
        .unwrap()
        .record_ids[0]
}

async fn parent(store: &PgControlPlaneStore, app: Uuid, id: Uuid) -> Option<Uuid> {
    store
        .application_log_record(app, id)
        .await
        .unwrap()
        .unwrap()
        .parent_task_run_id
}

#[tokio::test]
async fn agent_logs_hierarchy_late_parent_replay_and_late_ambiguity() {
    let (store, scope, app) = setup().await;
    let child_event = declared("child", "child-session", Some("parent"));
    let child = upload(&store, scope, app, child_event.clone()).await;
    assert_eq!(parent(&store, app, child).await, None);
    let root = upload(
        &store,
        scope,
        app,
        declared("parent", "parent-session", None),
    )
    .await;
    assert_eq!(parent(&store, app, child).await, Some(root));
    let before =
        serde_json::to_value(store.application_log_record(app, child).await.unwrap()).unwrap();
    let counts = owned_log_counts(&store, app, &[root, child], &[]).await;
    let replay = AgentLogsService::new(store.clone())
        .ingest(app, scope, Uuid::now_v7(), batch(vec![child_event]))
        .await
        .unwrap();
    assert_eq!((replay.accepted_events, replay.duplicate_events), (0, 1));
    assert_eq!(
        owned_log_counts(&store, app, &[root, child], &[]).await,
        counts
    );
    assert_eq!(
        serde_json::to_value(store.application_log_record(app, child).await.unwrap()).unwrap(),
        before
    );
    // Refreshing the child cannot clobber the good cross-session edge.
    let mut extra = declared("child", "child-session", Some("parent"));
    extra.event_id = "child-progress".into();
    extra.sequence = 2;
    extra.kind = AgentLogEventKind::Assistant;
    upload(&store, scope, app, extra).await;
    assert_eq!(parent(&store, app, child).await, Some(root));
    upload(
        &store,
        scope,
        app,
        declared("parent", "another-parent-session", None),
    )
    .await;
    assert_eq!(parent(&store, app, child).await, None);
    // Parent-first follows the same resolver.
    let first = upload(&store, scope, app, declared("first", "first-session", None)).await;
    let second = upload(
        &store,
        scope,
        app,
        declared("second", "second-session", Some("first")),
    )
    .await;
    assert_eq!(parent(&store, app, second).await, Some(first));
}

#[tokio::test]
async fn agent_logs_hierarchy_namespace_self_cycles_and_unbounded_depth() {
    let (store, scope, app) = setup().await;
    let child = upload(
        &store,
        scope,
        app,
        declared("child", "child", Some("foreign")),
    )
    .await;
    for (source, client) in [("other", "codex"), ("collector", "claude_code")] {
        let mut input = batch(vec![declared("foreign", client, None)]);
        input.source_id = source.into();
        input.source_client = client.into();
        AgentLogsService::new(store.clone())
            .ingest(app, scope, Uuid::now_v7(), input)
            .await
            .unwrap();
    }
    let owner: Uuid = sqlx::query_scalar("select created_by from applications where id=$1")
        .bind(app)
        .fetch_one(store.pool())
        .await
        .unwrap();
    let other_app = store
        .create_application(&CreateApplicationInput {
            actor_user_id: owner,
            workspace_id: scope,
            application_type: ApplicationType::AgentLogs,
            workflow_trigger_type: None,
            workflow_trigger_config: None,
            name: "foreign hierarchy".into(),
            description: "".into(),
            icon: None,
            icon_type: None,
            icon_background: None,
        })
        .await
        .unwrap();
    enable_reception_fixture(&store, scope, other_app.id).await;
    upload(
        &store,
        scope,
        other_app.id,
        declared("foreign", "foreign-app", None),
    )
    .await;
    assert_eq!(parent(&store, app, child).await, None);
    let foreign_scope = seed_workspace(&store, "hierarchy-foreign-scope").await;
    let displaced = upload(&store, scope, app, declared("foreign", "displaced", None)).await;
    sqlx::query("update application_run_log_tasks set scope_id=$2 where id=$1")
        .bind(displaced)
        .bind(foreign_scope)
        .execute(store.pool())
        .await
        .unwrap();
    let self_id = upload(&store, scope, app, declared("self", "self", Some("self"))).await;
    assert_eq!(parent(&store, app, child).await, None);
    assert_eq!(parent(&store, app, self_id).await, None);
    let a = upload(&store, scope, app, declared("a", "a", Some("b"))).await;
    let b = upload(&store, scope, app, declared("b", "b", Some("c"))).await;
    let c = upload(&store, scope, app, declared("c", "c", Some("a"))).await;
    for id in [a, b, c] {
        assert_eq!(parent(&store, app, id).await, None);
    }
    let events = (0..260)
        .map(|index| {
            declared(
                &format!("deep-{index}"),
                &format!("deep-session-{index}"),
                (index > 0)
                    .then(|| format!("deep-{}", index - 1))
                    .as_deref(),
            )
        })
        .collect();
    AgentLogsService::new(store.clone())
        .ingest(app, scope, Uuid::now_v7(), batch(events))
        .await
        .unwrap();
    let linked: i64 = sqlx::query_scalar("select count(*) from application_run_log_tasks where application_id=$1 and source_task_id like 'deep-%' and parent_task_run_id is not null").bind(app).fetch_one(store.pool()).await.unwrap();
    assert_eq!(linked, 259);
}

#[tokio::test]
async fn agent_logs_children_paging_nesting_membership_and_independent_trajectory() {
    let (store, scope, app) = setup().await;
    let root = upload(&store, scope, app, declared("root", "root", None)).await;
    let events = (0..107)
        .map(|index| {
            declared(
                &format!("child-{index:03}"),
                &format!("child-session-{index}"),
                Some("root"),
            )
        })
        .collect();
    AgentLogsService::new(store.clone())
        .ingest(app, scope, Uuid::now_v7(), batch(events))
        .await
        .unwrap();
    let expected: Vec<Uuid> = sqlx::query_scalar("select id from application_run_log_tasks where parent_task_run_id=$1 order by started_at,id").bind(root).fetch_all(store.pool()).await.unwrap();
    let child = expected[0];
    let child_name: String =
        sqlx::query_scalar("select source_task_id from application_run_log_tasks where id=$1")
            .bind(child)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let grandchild = upload(
        &store,
        scope,
        app,
        declared("grandchild", "nested", Some(&child_name)),
    )
    .await;
    let overview = store
        .application_log_record(app, root)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(overview.child_task_count, 107);
    let initial = store
        .application_log_record_children_page(app, root, None, None, 5)
        .await
        .unwrap();
    assert_eq!(
        initial
            .items
            .iter()
            .map(|i| i.record_id)
            .collect::<Vec<_>>(),
        expected[102..]
    );
    assert!(initial.page.has_before && !initial.page.has_after);
    let first = store
        .application_log_record_children_page(app, root, None, None, 100)
        .await
        .unwrap();
    assert_eq!(first.items.len(), 100);
    let older = store
        .application_log_record_children_page(app, root, first.page.before_cursor, None, 100)
        .await
        .unwrap();
    assert_eq!(older.items.len(), 7);
    let actual = older
        .items
        .iter()
        .chain(first.items.iter())
        .map(|i| i.record_id)
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
    let forward = store
        .application_log_record_children_page(app, root, None, Some(child), 5)
        .await
        .unwrap();
    assert_eq!(
        forward
            .items
            .iter()
            .map(|i| i.record_id)
            .collect::<Vec<_>>(),
        expected[1..6]
    );
    for invalid in [root, grandchild] {
        assert!(store
            .application_log_record_children_page(app, root, Some(invalid), None, 5)
            .await
            .is_err());
    }
    assert!(store
        .application_log_record_children_page(app, root, Some(child), Some(child), 5)
        .await
        .is_err());
    assert!(store
        .application_log_record_children_page(Uuid::now_v7(), root, None, None, 5)
        .await
        .is_err());
    let nested = store
        .application_log_record_children_page(app, child, None, None, 5)
        .await
        .unwrap();
    assert_eq!(nested.items[0].record_id, grandchild);
    assert!(store
        .application_log_record_children_page(app, grandchild, None, None, 5)
        .await
        .unwrap()
        .items
        .is_empty());
    let session = store
        .application_log_record_conversation_page(app, root, None, None, 5)
        .await
        .unwrap();
    assert_eq!(
        session
            .items
            .iter()
            .map(|i| i.record_id)
            .collect::<Vec<_>>(),
        vec![root]
    );
    let trace = store
        .record_client_trajectory_page(app, child, None, 5)
        .await
        .unwrap();
    assert_eq!(trace.items.len(), 1);
    assert_eq!(trace.items[0].turn_id.as_deref(), Some(child_name.as_str()));
    assert_eq!(trace.items[0].request_id, child);
    let raw = store
        .record_client_trajectory_section(app, child, trace.items[0].id, "raw", None, 5)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(raw.items[0].value["parent_source_task_id"], json!("root"));
    // Corrupted links still cannot leak another source/client/scope into paging.
    let foreign = upload(&store, scope, app, declared("foreign", "foreign", None)).await;
    sqlx::query("update application_run_log_tasks set parent_task_run_id=$2,source_client='foreign-client' where id=$1").bind(foreign).bind(root).execute(store.pool()).await.unwrap();
    assert_eq!(
        store
            .application_log_record(app, root)
            .await
            .unwrap()
            .unwrap()
            .child_task_count,
        107
    );
    assert!(store
        .application_log_record_children_page(app, root, Some(foreign), None, 5)
        .await
        .is_err());
    sqlx::query("update application_run_log_tasks set source_client='codex',source_id='foreign-source' where id=$1").bind(foreign).execute(store.pool()).await.unwrap();
    assert_eq!(
        store
            .application_log_record(app, root)
            .await
            .unwrap()
            .unwrap()
            .child_task_count,
        107
    );
    assert!(store
        .application_log_record_children_page(app, root, Some(foreign), None, 5)
        .await
        .is_err());
    let foreign_scope = seed_workspace(&store, "paging-foreign-scope").await;
    sqlx::query(
        "update application_run_log_tasks set source_id='collector',scope_id=$2 where id=$1",
    )
    .bind(foreign)
    .bind(foreign_scope)
    .execute(store.pool())
    .await
    .unwrap();
    assert_eq!(
        store
            .application_log_record(app, root)
            .await
            .unwrap()
            .unwrap()
            .child_task_count,
        107
    );
    assert!(store
        .application_log_record_children_page(app, root, Some(foreign), None, 5)
        .await
        .is_err());
}

#[tokio::test]
async fn agent_logs_imported_monitoring_sums_exclusive_tree_usage_once() {
    let (store, scope, app) = setup().await;
    for (task, declared_parent, tokens) in [
        ("root", None, 10),
        ("child", Some("root"), 20),
        ("grandchild", Some("child"), 30),
    ] {
        let mut e = declared(task, task, declared_parent);
        e.kind = AgentLogEventKind::TaskEnd;
        e.phase = Some("final_answer".into());
        e.usage = Some(AgentLogUsage {
            basis: AgentLogUsageBasis::Delta,
            response_id: None,
            input_tokens: Some(tokens),
            output_tokens: Some(0),
            input_cache_hit_tokens: Some(0),
            cache_write_tokens: None,
            total_tokens: Some(tokens),
        });
        let id = upload(&store, scope, app, e).await;
        assert_eq!(
            store
                .application_log_record(app, id)
                .await
                .unwrap()
                .unwrap()
                .total_tokens,
            Some(tokens)
        );
    }
    let report = store
        .get_application_run_monitoring_report(
            app,
            GetApplicationRunMonitoringReportInput {
                started_from: None,
                started_to: None,
                bucket: "hour".into(),
                slow_run_threshold_ms: 30_000,
            },
        )
        .await
        .unwrap();
    assert_eq!(report.overview.total_count, 1);
    assert_eq!(report.tokens.total_tokens_sum, 60);
    assert_eq!(report.tokens.input_tokens_sum, 60);
    assert_eq!(report.tokens.token_recorded_count, 1);
}

#[tokio::test]
async fn agent_logs_root_session_page_reserves_five_root_turn_slots() {
    let (store, scope, app) = setup().await;
    let mut roots = Vec::new();
    for index in 0..7 {
        roots.push(
            upload(
                &store,
                scope,
                app,
                declared(&format!("root-{index}"), "shared", None),
            )
            .await,
        );
    }
    roots.sort();
    let root_task: String =
        sqlx::query_scalar("select source_task_id from application_run_log_tasks where id=$1")
            .bind(roots[6])
            .fetch_one(store.pool())
            .await
            .unwrap();
    let child = upload(
        &store,
        scope,
        app,
        declared("same-session-child", "shared", Some(&root_task)),
    )
    .await;
    let initial = store
        .application_log_record_conversation_page(app, roots[6], None, None, 5)
        .await
        .unwrap();
    assert_eq!(
        initial
            .items
            .iter()
            .map(|item| item.record_id)
            .collect::<Vec<_>>(),
        roots[2..]
    );
    let older = store
        .application_log_record_conversation_page(
            app,
            roots[6],
            initial.page.before_cursor,
            None,
            5,
        )
        .await
        .unwrap();
    assert_eq!(
        older
            .items
            .iter()
            .map(|item| item.record_id)
            .collect::<Vec<_>>(),
        roots[..2]
    );
    assert!(store
        .application_log_record_conversation_page(app, roots[6], Some(child), None, 5)
        .await
        .is_err());
    assert_eq!(
        store
            .application_log_record_children_page(app, roots[6], None, None, 5)
            .await
            .unwrap()
            .items[0]
            .record_id,
        child
    );
}

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
    assert_eq!(
        store
            .application_log_record(app, child)
            .await
            .unwrap()
            .unwrap()
            .log_conversation_id,
        None
    );
    let root = upload(
        &store,
        scope,
        app,
        declared("parent", "parent-session", None),
    )
    .await;
    assert_eq!(parent(&store, app, child).await, Some(root));
    let canonical = store
        .application_log_record(app, root)
        .await
        .unwrap()
        .unwrap()
        .log_conversation_id;
    assert!(canonical.is_some());
    assert_eq!(
        store
            .application_log_record(app, child)
            .await
            .unwrap()
            .unwrap()
            .log_conversation_id,
        canonical
    );
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
    assert_eq!(
        store
            .application_log_record(app, child)
            .await
            .unwrap()
            .unwrap()
            .log_conversation_id,
        None
    );
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
        assert_eq!(
            store
                .application_log_record(app, id)
                .await
                .unwrap()
                .unwrap()
                .log_conversation_id,
            None
        );
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
async fn agent_logs_trace_metadata_paging_nesting_membership_and_independent_trajectory() {
    let (store, scope, app) = setup().await;
    let root = upload(&store, scope, app, declared("root", "root", None)).await;
    let events = (0..107)
        .map(|i| {
            declared(
                &format!("child-{i:03}"),
                &format!("child-session-{i}"),
                Some("root"),
            )
        })
        .collect();
    AgentLogsService::new(store.clone())
        .ingest(app, scope, Uuid::now_v7(), batch(events))
        .await
        .unwrap();
    let expected: Vec<Uuid> = sqlx::query_scalar(
        "select id from application_run_log_tasks where parent_task_run_id=$1 order by id",
    )
    .bind(root)
    .fetch_all(store.pool())
    .await
    .unwrap();
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
    let tree = store
        .application_log_record_trace_tree(app, root)
        .await
        .unwrap();
    assert_eq!(tree.nodes.len(), 1);
    assert_eq!(tree.nodes[0].record_id, root);
    assert_eq!(tree.nodes[0].child_count, 107);
    assert_eq!(tree.nodes[0].parent_trace_node_id, None);
    assert!(tree.nodes[0].has_content);
    let value = serde_json::to_value(&tree.nodes[0]).unwrap();
    assert!(
        value.get("messages").is_none()
            && value.get("raw").is_none()
            && value.get("flow_run_id").is_none()
    );
    let first = store
        .application_log_record_trace_children(app, root, root, None, 100)
        .await
        .unwrap();
    assert_eq!(first.items.len(), 100);
    assert!(first.page_info.has_more);
    assert!(first
        .items
        .iter()
        .all(|n| n.parent_trace_node_id == Some(root) && n.node_kind == "child_task"));
    let second = store
        .application_log_record_trace_children(
            app,
            root,
            root,
            first.page_info.next_cursor.clone(),
            100,
        )
        .await
        .unwrap();
    assert_eq!(second.items.len(), 7);
    assert!(!second.page_info.has_more);
    assert_eq!(
        first
            .items
            .iter()
            .chain(second.items.iter())
            .map(|n| n.record_id)
            .collect::<Vec<_>>(),
        expected
    );
    assert!(store
        .application_log_record_trace_children(app, root, child, first.page_info.next_cursor, 100)
        .await
        .is_err());
    assert!(store
        .application_log_record_trace_children(app, root, root, Some("forged".into()), 5)
        .await
        .is_err());
    assert!(store
        .application_log_record_trace_children(Uuid::now_v7(), root, root, None, 5)
        .await
        .is_err());
    let nested = store
        .application_log_record_trace_children(app, root, child, None, 5)
        .await
        .unwrap();
    assert_eq!(nested.items[0].record_id, grandchild);
    assert_eq!(nested.items[0].parent_trace_node_id, Some(child));
    assert!(store
        .application_log_record_trace_children(app, root, grandchild, None, 5)
        .await
        .unwrap()
        .items
        .is_empty());
    let own_tree = store
        .application_log_record_trace_tree(app, child)
        .await
        .unwrap();
    assert_eq!(own_tree.nodes[0].node_kind, "task_round");
    assert_eq!(own_tree.nodes[0].parent_trace_node_id, None);
    let session = store
        .application_log_record_conversation_page(app, child, None, None, 5)
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
    let foreign = upload(&store, scope, app, declared("foreign", "foreign", None)).await;
    for (client, source) in [("foreign-client", "collector"), ("codex", "foreign-source")] {
        sqlx::query("update application_run_log_tasks set parent_task_run_id=$2,source_client=$3,source_id=$4 where id=$1").bind(foreign).bind(root).bind(client).bind(source).execute(store.pool()).await.unwrap();
        assert!(store
            .application_log_record_trace_children(app, root, foreign, None, 5)
            .await
            .is_err());
        assert_eq!(
            store
                .application_log_record_trace_tree(app, root)
                .await
                .unwrap()
                .nodes[0]
                .child_count,
            107
        );
    }
    let foreign_scope = seed_workspace(&store, "trace-foreign-scope").await;
    sqlx::query("update application_run_log_tasks set source_client='codex',source_id='collector',scope_id=$2 where id=$1").bind(foreign).bind(foreign_scope).execute(store.pool()).await.unwrap();
    assert!(store
        .application_log_record_trace_children(app, root, foreign, None, 5)
        .await
        .is_err());
    assert_eq!(
        store
            .application_log_record_trace_tree(app, root)
            .await
            .unwrap()
            .nodes[0]
            .child_count,
        107
    );
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
    let child_anchor = store
        .application_log_record_conversation_page(app, child, None, None, 5)
        .await
        .unwrap();
    assert_eq!(
        child_anchor
            .items
            .iter()
            .map(|i| i.record_id)
            .collect::<Vec<_>>(),
        roots[2..]
    );
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
            .application_log_record_trace_children(app, roots[6], roots[6], None, 5)
            .await
            .unwrap()
            .items[0]
            .record_id,
        child
    );
}

#[tokio::test]
async fn agent_logs_conversations_grouped_paging_filter_usage_and_backfill() {
    let (store, scope, app) = setup().await;
    let root = upload(&store, scope, app, declared("main-1", "main", None)).await;
    let latest = upload(&store, scope, app, declared("main-2", "main", None)).await;
    let child = upload(
        &store,
        scope,
        app,
        declared("child", "child", Some("main-1")),
    )
    .await;
    let unresolved = upload(
        &store,
        scope,
        app,
        declared("orphan", "orphan", Some("missing")),
    )
    .await;
    let other = upload(&store, scope, app, declared("other", "other", None)).await;
    sqlx::query("update application_run_log_tasks set total_tokens=case when id=$1 then 10 when id=$2 then 20 when id=$3 then 30 else 999 end,total_cost=case when id=$1 then 1 when id=$2 then 2 when id=$3 then 3 else 999 end where application_id=$4")
        .bind(root).bind(latest).bind(child).bind(app).execute(store.pool()).await.unwrap();
    let query = ApplicationLogConversationsQuery {
        filter: domain::ResourceFilterExpr::All(vec![]),
        sort_field: "total_tokens".into(),
        descending: false,
        page: 1,
        page_size: 1,
    };
    let first = store
        .query_application_log_conversations(scope, &[app], &query)
        .await
        .unwrap();
    assert_eq!(first.total, 2);
    assert_eq!(first.items.len(), 1);
    assert_eq!(first.items[0].record_id, latest);
    assert_eq!(first.items[0].source_session_id.as_deref(), Some("main"));
    assert_eq!(first.items[0].total_tokens, Some(60));
    assert_eq!(
        first.items[0]
            .total_cost
            .as_ref()
            .unwrap()
            .parse::<f64>()
            .unwrap(),
        6.0
    );
    assert!(first.items[0].log_conversation_id.is_some());
    let filtered = store
        .query_application_log_conversations(
            scope,
            &[app],
            &ApplicationLogConversationsQuery {
                filter: domain::ResourceFilterExpr::Field {
                    field: "id".into(),
                    operator: domain::ResourceFilterOperator::Eq,
                    value: json!(root),
                },
                ..query.clone()
            },
        )
        .await
        .unwrap();
    assert_eq!(filtered.total, 1);
    assert_eq!(filtered.items[0].record_id, latest);
    assert_eq!(filtered.items[0].total_tokens, Some(60));
    let child_filter = store
        .query_application_log_conversations(
            scope,
            &[app],
            &ApplicationLogConversationsQuery {
                filter: domain::ResourceFilterExpr::Field {
                    field: "id".into(),
                    operator: domain::ResourceFilterOperator::Eq,
                    value: json!(child),
                },
                ..query.clone()
            },
        )
        .await
        .unwrap();
    assert_eq!(child_filter.total, 0);
    let source_filter = store
        .query_application_log_conversations(
            scope,
            &[app],
            &ApplicationLogConversationsQuery {
                filter: domain::ResourceFilterExpr::Field {
                    field: "source_id".into(),
                    operator: domain::ResourceFilterOperator::Eq,
                    value: json!("foreign-source"),
                },
                ..query.clone()
            },
        )
        .await
        .unwrap();
    assert_eq!(source_filter.total, 0);
    assert!(store
        .query_application_log_conversations(
            scope,
            &[app],
            &ApplicationLogConversationsQuery {
                sort_field: "raw".into(),
                ..query.clone()
            }
        )
        .await
        .is_err());
    assert!(store
        .query_application_log_conversations(
            scope,
            &[app],
            &ApplicationLogConversationsQuery {
                page: i64::MAX,
                page_size: 2,
                ..query.clone()
            }
        )
        .await
        .is_err());
    let large = store
        .query_application_log_conversations(
            scope,
            &[app],
            &ApplicationLogConversationsQuery {
                page_size: 107,
                ..query.clone()
            },
        )
        .await
        .unwrap();
    assert_eq!(large.items.len(), 2);
    let second = store
        .query_application_log_conversations(
            scope,
            &[app],
            &ApplicationLogConversationsQuery {
                page: 2,
                ..query.clone()
            },
        )
        .await
        .unwrap();
    assert_eq!(second.items[0].record_id, other);
    let past = store
        .query_application_log_conversations(
            scope,
            &[app],
            &ApplicationLogConversationsQuery {
                page: 3,
                ..query.clone()
            },
        )
        .await
        .unwrap();
    assert_eq!(past.total, 2);
    assert!(past.items.is_empty());
    assert_eq!(
        store
            .query_application_log_conversations(Uuid::now_v7(), &[app], &query)
            .await
            .unwrap()
            .total,
        0
    );
    assert_eq!(
        store
            .query_application_log_conversations(scope, &[Uuid::now_v7()], &query)
            .await
            .unwrap()
            .total,
        0
    );
    let normalized = store
        .application_log_record_conversation_page(app, child, None, None, 5)
        .await
        .unwrap();
    assert_eq!(
        normalized
            .items
            .iter()
            .map(|i| i.record_id)
            .collect::<Vec<_>>(),
        vec![root, latest]
    );
    assert!(store
        .application_log_record_conversation_page(app, root, Some(child), None, 5)
        .await
        .is_err());
    assert_eq!(
        store
            .application_log_record(app, unresolved)
            .await
            .unwrap()
            .unwrap()
            .log_conversation_id,
        None
    );
    // Backfill projection recomputation keeps observation identities and bodies intact.
    let before =
        owned_log_counts(&store, app, &[root, latest, child, unresolved, other], &[]).await;
    sqlx::query(
        "update application_run_log_tasks set log_conversation_id=null where application_id=$1",
    )
    .bind(app)
    .execute(store.pool())
    .await
    .unwrap();
    sqlx::query("select refresh_imported_conversation_ownership(null,null,null,null)")
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        owned_log_counts(&store, app, &[root, latest, child, unresolved, other], &[]).await,
        before
    );
    assert_eq!(
        store
            .application_log_record(app, child)
            .await
            .unwrap()
            .unwrap()
            .log_conversation_id,
        first.items[0].log_conversation_id
    );
}

#[tokio::test]
async fn agent_logs_canonical_conversation_keeps_identity_and_first_main_title_fresh() {
    let (store, scope, app) = setup().await;
    let root = upload(&store, scope, app, declared("first question", "main", None)).await;
    let canonical = store
        .application_log_record(app, root)
        .await
        .unwrap()
        .unwrap()
        .log_conversation_id
        .unwrap();
    sqlx::query(
        "update application_conversations set updated_at='2000-01-01T00:00:00Z' where id=$1",
    )
    .bind(canonical)
    .execute(store.pool())
    .await
    .unwrap();
    let next = upload(&store, scope, app, declared("later question", "main", None)).await;
    assert_eq!(
        store
            .application_log_record(app, next)
            .await
            .unwrap()
            .unwrap()
            .log_conversation_id,
        Some(canonical)
    );
    let (title,fresh):(String,bool)=sqlx::query_as("select title,updated_at>'2000-01-01T00:00:00Z'::timestamptz from application_conversations where id=$1").bind(canonical).fetch_one(store.pool()).await.unwrap();
    assert_eq!(title, "first question");
    assert!(fresh);
    let count:i64=sqlx::query_scalar("select count(*) from application_conversations where application_id=$1 and source_kind='imported' and source_session_id='main'").bind(app).fetch_one(store.pool()).await.unwrap();
    assert_eq!(count, 1);
}

use super::*;
use domain::{ResourceFilterExpr as F, ResourceFilterOperator as O};

fn query() -> ConversationClientTrajectoryQuery {
    ConversationClientTrajectoryQuery {
        filter: F::All(vec![]),
        cursor: None,
        limit: 1,
        keyword: None,
        search_sections: vec![],
        focus_record_id: None,
    }
}

#[tokio::test]
async fn canonical_timeline_interleaves_descendants_bidirectionally_and_focus_is_not_scope() {
    let (store, scope, app) = setup().await;
    let service = AgentLogsService::new(store.clone());
    let mut events = vec![];
    for (id, task, parent, timestamp) in [
        ("main-first", "main", None, "2026-10-07T08:00:00.000000001Z"),
        (
            "child",
            "child",
            Some("main"),
            "2026-10-07T08:00:00.000000002Z",
        ),
        (
            "grandchild",
            "grandchild",
            Some("child"),
            "2026-10-07T08:00:00.000000002Z",
        ),
        (
            "main-second",
            "second",
            None,
            "2026-10-07T08:00:00.000000003Z",
        ),
    ] {
        let mut e = event(
            id,
            1,
            AgentLogEventKind::ToolResult,
            Some("semantic needle"),
        );
        if parent.is_some() {
            e.content = Some(format!("{} semantic needle", "padding ".repeat(100)));
        }
        e.raw = json!({"archive_only":"raw archive needle"});
        e.source_task_id = task.into();
        e.parent_source_task_id = parent.map(Into::into);
        e.occurred_at = timestamp.into();
        events.push(e);
    }
    // Reverse ingestion ensures ordering cannot accidentally pass using insertion UUIDs.
    events.reverse();
    let receipt = service
        .ingest(app, scope, Uuid::now_v7(), batch(events))
        .await
        .unwrap();
    let conversation: Uuid =
        sqlx::query_scalar("select log_conversation_id from application_run_log_tasks where id=$1")
            .bind(receipt.record_ids[0])
            .fetch_one(store.pool())
            .await
            .unwrap();
    let mut facts: Vec<(String,Uuid)> = sqlx::query_as("select s.observed_at,s.id from client_trajectory_steps s join application_run_log_tasks t on t.id=s.record_id where t.application_id=$1")
        .bind(app).fetch_all(store.pool()).await.unwrap();
    facts.sort_by_key(|(timestamp, id)| {
        (
            OffsetDateTime::parse(timestamp, &time::format_description::well_known::Rfc3339)
                .unwrap(),
            *id,
        )
    });
    let expected: Vec<Uuid> = facts.into_iter().map(|(_, id)| id).collect();
    let mut q = query();
    let mut pages = vec![];
    loop {
        let page = store
            .query_conversation_client_trajectory(app, conversation, &q)
            .await
            .unwrap();
        assert_eq!(page.items.len(), 1);
        pages.push(page.clone());
        match page.next_cursor {
            Some(cursor) => q.cursor = Some(cursor),
            None => break,
        }
    }
    assert_eq!(
        pages.iter().map(|p| p.items[0].step.id).collect::<Vec<_>>(),
        expected
    );
    for index in (1..pages.len()).rev() {
        q.cursor = pages[index].previous_cursor.clone();
        let previous = store
            .query_conversation_client_trajectory(app, conversation, &q)
            .await
            .unwrap();
        assert_eq!(previous.items[0].step.id, expected[index - 1]);
    }
    let mut q = query();
    q.focus_record_id = Some(pages.last().unwrap().items[0].record_id);
    let focused = store
        .query_conversation_client_trajectory(app, conversation, &q)
        .await
        .unwrap();
    assert_eq!(
        focused.items[0].step.item_id.as_deref(),
        Some("main-second")
    );
    assert!(focused.previous_cursor.is_some());
    q.cursor = focused.previous_cursor;
    assert!(!store
        .query_conversation_client_trajectory(app, conversation, &q)
        .await
        .unwrap()
        .items
        .is_empty());
    q = query();
    q.filter = F::Field {
        field: "is_subagent".into(),
        operator: O::Eq,
        value: json!(true),
    };
    q.keyword = Some("semantic needle".into());
    q.limit = 10;
    let children = store
        .query_conversation_client_trajectory(app, conversation, &q)
        .await
        .unwrap();
    assert_eq!(children.items.len(), 2);
    assert!(children
        .items
        .iter()
        .all(|s| s.is_subagent && s.step.category == "tool_result"));
    assert_eq!(children.matches.len(), 2);
    let mut bounded = query();
    bounded.limit = 20;
    bounded.filter = F::All(vec![
        F::Field {
            field: "category".into(),
            operator: O::Eq,
            value: json!("tool_result"),
        },
        F::Field {
            field: "created_at".into(),
            operator: O::Gt,
            value: json!("2026-10-07T08:00:00.000000001Z"),
        },
        F::Field {
            field: "created_at".into(),
            operator: O::Lt,
            value: json!("2026-10-07T16:00:00.000000003+08:00"),
        },
    ]);
    assert_eq!(
        store
            .query_conversation_client_trajectory(app, conversation, &bounded)
            .await
            .unwrap()
            .items
            .len(),
        2
    );
    let mut raw_only = query();
    raw_only.keyword = Some("raw archive needle".into());
    assert!(store
        .query_conversation_client_trajectory(app, conversation, &raw_only)
        .await
        .unwrap()
        .items
        .is_empty());
    raw_only.search_sections = vec!["overview".into()];
    assert!(store
        .query_conversation_client_trajectory(app, conversation, &raw_only)
        .await
        .unwrap()
        .items
        .is_empty());
    raw_only.search_sections = vec!["raw".into()];
    assert!(store
        .query_conversation_client_trajectory(app, conversation, &raw_only)
        .await
        .is_err());
    let main_round = pages.last().unwrap().items[0].record_id;
    let mut focused_children = query();
    focused_children.focus_record_id = Some(main_round);
    focused_children.filter = F::Field {
        field: "is_subagent".into(),
        operator: O::Eq,
        value: json!(true),
    };
    focused_children.keyword = Some("semantic needle".into());
    let older_children = store
        .query_conversation_client_trajectory(app, conversation, &focused_children)
        .await
        .unwrap();
    assert_eq!(older_children.items.len(), 1);
    assert!(older_children.items[0].is_subagent);
    assert!(older_children.previous_cursor.is_some());
    assert!(older_children.next_cursor.is_none());
    focused_children.cursor = older_children.previous_cursor;
    let earlier_child = store
        .query_conversation_client_trajectory(app, conversation, &focused_children)
        .await
        .unwrap();
    assert_eq!(earlier_child.items.len(), 1);
    assert_ne!(
        earlier_child.items[0].step.id,
        older_children.items[0].step.id
    );
    q = query();
    q.focus_record_id = Some(Uuid::now_v7());
    assert!(store
        .query_conversation_client_trajectory(app, conversation, &q)
        .await
        .is_err());
    assert!(store
        .query_conversation_client_trajectory(Uuid::now_v7(), conversation, &query())
        .await
        .is_err());
    let step = pages[0].items[0].step.id;
    assert!(store
        .read_conversation_client_trajectory_section(app, Uuid::now_v7(), step, "result", None, 32)
        .await
        .is_err());
    assert!(store
        .read_conversation_client_trajectory_section(app, conversation, step, "result", None, 32)
        .await
        .unwrap()
        .is_some());
    let mut mismatched = query();
    mismatched.keyword = Some("changed".into());
    mismatched.cursor = pages[0].next_cursor.clone();
    assert!(store
        .query_conversation_client_trajectory(app, conversation, &mismatched)
        .await
        .is_err());
    let mut tampered = query();
    tampered.cursor = Some("not-a-cursor".into());
    assert!(store
        .query_conversation_client_trajectory(app, conversation, &tampered)
        .await
        .is_err());
    // A valid selected round survives absent metadata events; task.started_at
    // supplies truthful positioning while the entire canonical scope stays readable.
    sqlx::query("delete from client_trajectory_steps where record_id=$1")
        .bind(main_round)
        .execute(store.pool())
        .await
        .unwrap();
    let mut empty_round = query();
    empty_round.focus_record_id = Some(main_round);
    assert!(!store
        .query_conversation_client_trajectory(app, conversation, &empty_round)
        .await
        .unwrap()
        .items
        .is_empty());
    let mut late = event(
        "late-history",
        999,
        AgentLogEventKind::ToolResult,
        Some("late"),
    );
    late.occurred_at = "2026-10-07T07:00:00Z".into();
    late.source_task_id = "main".into();
    service
        .ingest(app, scope, Uuid::now_v7(), batch(vec![late]))
        .await
        .unwrap();
    let refreshed = store
        .query_conversation_client_trajectory(app, conversation, &query())
        .await
        .unwrap();
    assert_eq!(
        refreshed.items[0].step.item_id.as_deref(),
        Some("late-history")
    );
}

#[tokio::test]
async fn canonical_timeline_namespace_scope_and_unresolved_membership_are_closed() {
    let (store, scope, app) = setup().await;
    let service = AgentLogsService::new(store.clone());
    let first = service
        .ingest(
            app,
            scope,
            Uuid::now_v7(),
            batch(vec![event(
                "owned",
                1,
                AgentLogEventKind::User,
                Some("owned"),
            )]),
        )
        .await
        .unwrap()
        .record_ids[0];
    let conversation: Uuid =
        sqlx::query_scalar("select log_conversation_id from application_run_log_tasks where id=$1")
            .bind(first)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let mut foreign_batch = batch(vec![event(
        "foreign",
        1,
        AgentLogEventKind::User,
        Some("foreign"),
    )]);
    foreign_batch.source_id = "foreign-namespace".into();
    let foreign = service
        .ingest(app, scope, Uuid::now_v7(), foreign_batch)
        .await
        .unwrap()
        .record_ids[0];
    let foreign_step: Uuid =
        sqlx::query_scalar("select id from client_trajectory_steps where record_id=$1")
            .bind(foreign)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let mut q = query();
    q.focus_record_id = Some(foreign);
    assert!(store
        .query_conversation_client_trajectory(app, conversation, &q)
        .await
        .is_err());
    assert!(store
        .read_conversation_client_trajectory_section(
            app,
            conversation,
            foreign_step,
            "raw",
            None,
            1
        )
        .await
        .is_err());
    // A corrupted UUID membership alone must not bypass source namespace checks.
    sqlx::query("update application_run_log_tasks set log_conversation_id=$1 where id=$2")
        .bind(conversation)
        .bind(foreign)
        .execute(store.pool())
        .await
        .unwrap();
    let mut q = query();
    q.limit = 50;
    let page = store
        .query_conversation_client_trajectory(app, conversation, &q)
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].record_id, first);
    assert!(store
        .read_conversation_client_trajectory_section(
            app,
            conversation,
            foreign_step,
            "raw",
            None,
            1
        )
        .await
        .is_err());
    // The application scope is authoritative, even if canonical UUIDs agree.
    let other_scope = seed_workspace(&store, "timeline-other-scope").await;
    sqlx::query("update application_run_log_tasks set scope_id=$1 where id=$2")
        .bind(other_scope)
        .bind(first)
        .execute(store.pool())
        .await
        .unwrap();
    assert!(store
        .query_conversation_client_trajectory(app, conversation, &q)
        .await
        .unwrap()
        .items
        .is_empty());
    let mut unresolved = event("unresolved", 1, AgentLogEventKind::User, Some("unresolved"));
    unresolved.parent_source_task_id = Some("missing-parent".into());
    unresolved.source_task_id = "unresolved-child".into();
    let id = service
        .ingest(app, scope, Uuid::now_v7(), batch(vec![unresolved]))
        .await
        .unwrap()
        .record_ids[0];
    let canonical: Option<Uuid> =
        sqlx::query_scalar("select log_conversation_id from application_run_log_tasks where id=$1")
            .bind(id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(canonical, None);
    q.focus_record_id = Some(id);
    assert!(store
        .query_conversation_client_trajectory(app, conversation, &q)
        .await
        .is_err());
}

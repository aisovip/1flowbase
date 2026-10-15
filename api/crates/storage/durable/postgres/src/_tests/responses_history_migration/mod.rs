//! Repository-only COW/ownership fixtures. Protocol proof trust is exercised by
//! the separate control-plane/PostgreSQL suite, never fabricated in this adapter.
use super::*;

async fn fixture() -> (PgControlPlaneStore, ResponsesHistoryFlowKey) {
    let (pool, flow_run_id) =
        crate::_tests::provider_protocol_capsule_store_tests::seeded_flow_run().await;
    let store = PgControlPlaneStore::new(pool);
    sqlx::query("update flow_runs set status='paused' where id=$1")
        .bind(flow_run_id)
        .execute(store.pool())
        .await
        .unwrap();
    let (scope_id, application_id) = sqlx::query_as::<_, (Uuid, Uuid)>(
        "select scope_id,application_id from flow_runs where id=$1",
    )
    .bind(flow_run_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    (
        store,
        ResponsesHistoryFlowKey {
            scope_id,
            application_id,
            flow_run_id,
        },
    )
}

async fn append(
    store: &PgControlPlaneStore,
    flow: &ResponsesHistoryFlowKey,
    content: Value,
    parent: Option<Uuid>,
) -> ResponsesHistoryContextLineageEntry {
    let mut tx = store.pool().begin().await.unwrap();
    let entry = contexts::append(&mut tx, flow, &content, parent)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    entry
}

async fn checkpoint(
    store: &PgControlPlaneStore,
    flow: &ResponsesHistoryFlowKey,
    head: Option<&ResponsesHistoryContextLineageEntry>,
    pool: Value,
) -> Uuid {
    let mut locator = json!({"node_id":"fixture-node","untouched":"locator\0value"});
    let snapshot = if let Some(head) = head {
        locator["context_version_id"] = json!(head.identity.context_version_id);
        json!({"__runtime_recovery_context":{"context_version_id":head.identity.context_version_id,"sequence":head.sequence}})
    } else {
        pool
    };
    store
        .create_checkpoint(&CreateCheckpointInput {
            flow_run_id: flow.flow_run_id,
            node_run_id: None,
            status: "paused".into(),
            reason: "dag adapter fixture".into(),
            locator_payload: locator,
            variable_snapshot: snapshot,
            external_ref_payload: None,
        })
        .await
        .unwrap()
        .id
}

async fn count_versions(store: &PgControlPlaneStore, flow: &ResponsesHistoryFlowKey) -> i64 {
    sqlx::query_scalar("select count(*) from runtime_context_projections where flow_run_id=$1")
        .bind(flow.flow_run_id)
        .fetch_one(store.pool())
        .await
        .unwrap()
}

fn shared_plan(snapshot: &ResponsesHistoryFlowSnapshot) -> ResponsesHistoryMigrationPlan {
    let contexts = snapshot
        .contexts
        .iter()
        .enumerate()
        .map(|(index, before)| {
            let mut after_content = before.content.clone();
            if index == 0 {
                after_content["variable_pool"]["owned_marker"] = json!("after");
            }
            ResponsesHistoryContextChange {
                before: before.clone(),
                after_content,
            }
        })
        .collect();
    ResponsesHistoryMigrationPlan {
        flow: snapshot.flow,
        expected_status: snapshot.status,
        expected_latest_context_version_id: snapshot.latest_context_version_id,
        documents: vec![],
        contexts,
        checkpoints: snapshot
            .checkpoints
            .iter()
            .map(|checkpoint| ResponsesHistoryCheckpointChange {
                checkpoint_id: checkpoint.checkpoint_id,
                before_locator_payload: checkpoint.locator_payload.clone(),
                before_variable_snapshot: checkpoint.variable_snapshot.clone(),
                before_context: checkpoint.context.clone(),
                legacy_inline_variable_pool: None,
            })
            .collect(),
        continuations: vec![],
    }
}

#[tokio::test]
async fn responses_history_dag_shared_ancestors_are_loaded_and_copied_once() {
    let (store, flow) = fixture().await;
    let original_blob = format!("{}\0end", "long-tool-output".repeat(1024));
    let mut head=append(&store,&flow,json!({"format":"runtime_snapshot_v1","variable_pool":{"owned_marker":"before","user":{"blob":original_blob}}}),None).await;
    const VERSIONS: usize = 24;
    for index in 0..VERSIONS {
        if index > 0 {
            head=append(&store,&flow,json!({"format":"runtime_delta_v1","set":{format!("step-{index}"):index},"remove":[],"llm_callback_appends":[]}),Some(head.identity.context_version_id)).await;
        }
        // Two owners share every head; neither loads nor writes may duplicate it.
        checkpoint(&store, &flow, Some(&head), json!({})).await;
        checkpoint(&store, &flow, Some(&head), json!({})).await;
    }
    let before = store.load_responses_history_flow(&flow, "").await.unwrap();
    assert_eq!(before.contexts.len(), VERSIONS);
    assert_eq!(before.checkpoints.len(), 2 * VERSIONS);
    let unique: HashSet<_> = before
        .contexts
        .iter()
        .map(|c| c.identity.context_version_id)
        .collect();
    assert_eq!(unique.len(), VERSIONS);
    let plan = shared_plan(&before);
    store
        .preview_responses_history_migration(&plan, "")
        .await
        .unwrap();
    assert_eq!(count_versions(&store, &flow).await, VERSIONS as i64);
    let receipt = store
        .apply_responses_history_migration(&plan, "")
        .await
        .unwrap();
    assert_eq!(receipt.contexts.len(), VERSIONS);
    assert_eq!(count_versions(&store, &flow).await, 2 * VERSIONS as i64);
    let after = store.load_responses_history_flow(&flow, "").await.unwrap();
    assert_eq!(after.contexts.len(), VERSIONS);
    assert_eq!(
        after.contexts[0].content["variable_pool"]["user"]["blob"],
        original_blob
    );
    assert_eq!(
        after.contexts[0].content["variable_pool"]["owned_marker"],
        "after"
    );
    assert_eq!(
        after
            .contexts
            .iter()
            .filter(|c| c.content["format"] == "runtime_snapshot_v1")
            .count(),
        1
    );
    for (old, new) in before.contexts.iter().zip(&after.contexts).skip(1) {
        assert_eq!(old.content, new.content, "delta encoding must survive COW");
        assert_ne!(
            old.identity.context_version_id,
            new.identity.context_version_id
        );
    }
    let old_bytes: usize = before
        .contexts
        .iter()
        .map(|c| serde_json::to_vec(&c.content).unwrap().len())
        .sum();
    let new_bytes: usize = after
        .contexts
        .iter()
        .map(|c| serde_json::to_vec(&c.content).unwrap().len())
        .sum();
    assert!(
        new_bytes <= old_bytes + 32,
        "no repeated full pools in each checkpoint"
    );
    let new_ids: HashSet<_> = receipt
        .contexts
        .iter()
        .map(|c| c.after.context_version_id)
        .collect();
    assert!(after
        .checkpoints
        .iter()
        .all(|c| new_ids.contains(&c.context.as_ref().unwrap().context_version_id)));
    for old in &before.contexts {
        let stored = contexts::context(
            &mut store.pool().begin().await.unwrap(),
            &flow,
            old.identity.context_version_id,
        )
        .await
        .unwrap()
        .entry;
        assert_eq!(&stored, old, "old immutable DAG must remain untouched");
    }
    let replay = store
        .apply_responses_history_migration(&plan, "")
        .await
        .unwrap();
    assert!(replay.contexts.is_empty() && replay.checkpoints.is_empty());
    assert_eq!(count_versions(&store, &flow).await, 2 * VERSIONS as i64);
    store
        .rollback_responses_history_migration(&receipt, "")
        .await
        .unwrap();
    let restored = store.load_responses_history_flow(&flow, "").await.unwrap();
    assert_eq!(restored.checkpoints, before.checkpoints);
    assert_eq!(
        count_versions(&store, &flow).await,
        2 * VERSIONS as i64,
        "rollback retains immutable audit nodes"
    );
}

#[tokio::test]
async fn responses_history_dag_disconnected_latest_anchor_stays_latest_and_stale_rollback_refuses()
{
    let (store, flow) = fixture().await;
    let old = append(
        &store,
        &flow,
        json!({"format":"runtime_snapshot_v1","variable_pool":{"owned_marker":"before"}}),
        None,
    )
    .await;
    checkpoint(&store, &flow, Some(&old), json!({})).await;
    let latest = append(
        &store,
        &flow,
        json!({"format":"runtime_snapshot_v1","variable_pool":{"current":"independent branch"}}),
        None,
    )
    .await;
    let before = store.load_responses_history_flow(&flow, "").await.unwrap();
    assert_eq!(
        before.contexts.len(),
        2,
        "latest root is loaded even without checkpoint reference"
    );
    let plan = shared_plan(&before);
    let receipt = store
        .apply_responses_history_migration(&plan, "")
        .await
        .unwrap();
    let new_latest = receipt.after_latest_context.as_ref().unwrap();
    assert_eq!(receipt.contexts.last().unwrap().before, latest.identity);
    assert_eq!(receipt.contexts.last().unwrap().after, *new_latest);
    let after = store.load_responses_history_flow(&flow, "").await.unwrap();
    assert_eq!(after.contexts.last().unwrap().content, latest.content);
    assert_eq!(
        after.latest_context_version_id,
        Some(new_latest.context_version_id)
    );
    let old_checkpoint = &before.checkpoints[0];
    store
        .update_checkpoint_payloads(&UpdateCheckpointPayloadsInput {
            checkpoint_id: old_checkpoint.checkpoint_id,
            locator_payload: old_checkpoint.locator_payload.clone(),
            variable_snapshot: old_checkpoint.variable_snapshot.clone(),
            external_ref_payload: None,
        })
        .await
        .unwrap();
    assert!(
        store
            .apply_responses_history_migration(&plan, "")
            .await
            .is_err(),
        "mixed old checkpoint and copied head must not be accepted"
    );
    assert_eq!(count_versions(&store, &flow).await, 4);
    let mut external = after.checkpoints[0].locator_payload.clone();
    external["later"] = json!(true);
    store
        .update_checkpoint_payloads(&UpdateCheckpointPayloadsInput {
            checkpoint_id: old_checkpoint.checkpoint_id,
            locator_payload: external,
            variable_snapshot: after.checkpoints[0].variable_snapshot.clone(),
            external_ref_payload: None,
        })
        .await
        .unwrap();
    assert!(store
        .rollback_responses_history_migration(&receipt, "")
        .await
        .is_err());
    let still = store.load_responses_history_flow(&flow, "").await.unwrap();
    assert_eq!(
        still.latest_context_version_id,
        after.latest_context_version_id
    );
    assert_eq!(still.checkpoints[0].context, after.checkpoints[0].context);
    assert_eq!(still.checkpoints[0].locator_payload["later"], true);
}

#[tokio::test]
async fn responses_history_dag_missing_descendant_and_latest_anchors_fail_before_writes() {
    let (store, flow) = fixture().await;
    let root = append(
        &store,
        &flow,
        json!({"format":"runtime_snapshot_v1","variable_pool":{"owned_marker":"before"}}),
        None,
    )
    .await;
    let child = append(
        &store,
        &flow,
        json!({"format":"runtime_delta_v1","set":{"child":1},"remove":[]}),
        Some(root.identity.context_version_id),
    )
    .await;
    checkpoint(&store, &flow, Some(&child), json!({})).await;
    let before = store.load_responses_history_flow(&flow, "").await.unwrap();
    let mut missing = shared_plan(&before);
    missing.contexts.pop();
    assert!(store
        .apply_responses_history_migration(&missing, "")
        .await
        .is_err());
    assert_eq!(count_versions(&store, &flow).await, 2);
    let mut reordered = shared_plan(&before);
    reordered.contexts.swap(0, 1);
    assert!(store
        .preview_responses_history_migration(&reordered, "")
        .await
        .is_err());
    let mut changed = shared_plan(&before);
    changed.contexts[0].before.content["variable_pool"]["owned_marker"] = json!("forged preimage");
    assert!(store
        .apply_responses_history_migration(&changed, "")
        .await
        .is_err());
    assert_eq!(count_versions(&store, &flow).await, 2);
    let after = store.load_responses_history_flow(&flow, "").await.unwrap();
    assert_eq!(after.contexts, before.contexts);
    assert_eq!(after.checkpoints, before.checkpoints);
}

#[tokio::test]
async fn responses_history_dag_legacy_inline_uses_existing_latest_checkpoint_order() {
    let (store, flow) = fixture().await;
    let pool = json!({"owned_marker":"before","user":"inline\0value"});
    checkpoint(&store, &flow, None, pool.clone()).await;
    checkpoint(
        &store,
        &flow,
        None,
        json!({"latest":"unchanged legacy pool"}),
    )
    .await;
    let before = store.load_responses_history_flow(&flow, "").await.unwrap();
    assert!(before.contexts.is_empty());
    let mut changes: Vec<_> = before
        .checkpoints
        .iter()
        .map(|c| ResponsesHistoryCheckpointChange {
            checkpoint_id: c.checkpoint_id,
            before_locator_payload: c.locator_payload.clone(),
            before_variable_snapshot: c.variable_snapshot.clone(),
            before_context: None,
            legacy_inline_variable_pool: Some(c.variable_snapshot.clone()),
        })
        .collect();
    changes[0].legacy_inline_variable_pool.as_mut().unwrap()["owned_marker"] = json!("after");
    let mut plan = ResponsesHistoryMigrationPlan {
        flow,
        expected_status: before.status,
        expected_latest_context_version_id: None,
        documents: vec![],
        contexts: vec![],
        checkpoints: changes,
        continuations: vec![],
    };
    let latest_anchor = plan.checkpoints.pop().unwrap();
    assert!(store
        .apply_responses_history_migration(&plan, "")
        .await
        .is_err());
    assert_eq!(count_versions(&store, &flow).await, 0);
    plan.checkpoints.push(latest_anchor);
    let receipt = store
        .apply_responses_history_migration(&plan, "")
        .await
        .unwrap();
    assert!(
        receipt.contexts.is_empty(),
        "legacy nodes have no fabricated old context mapping"
    );
    assert_eq!(
        receipt.checkpoints.last().unwrap().after_context,
        receipt.after_latest_context.clone().unwrap()
    );
    let after = store.load_responses_history_flow(&flow, "").await.unwrap();
    assert_eq!(after.contexts.len(), 2);
    assert_eq!(
        after.contexts[0].content["variable_pool"]["user"],
        "inline\0value"
    );
    assert_eq!(
        after.contexts.last().unwrap().content["variable_pool"],
        before.checkpoints.last().unwrap().variable_snapshot
    );
    assert!(store
        .apply_responses_history_migration(&plan, "")
        .await
        .unwrap()
        .checkpoints
        .is_empty());
    store
        .rollback_responses_history_migration(&receipt, "")
        .await
        .unwrap();
    let restored = store.load_responses_history_flow(&flow, "").await.unwrap();
    assert_eq!(restored.checkpoints, before.checkpoints);
    assert_eq!(count_versions(&store, &flow).await, 2);
}

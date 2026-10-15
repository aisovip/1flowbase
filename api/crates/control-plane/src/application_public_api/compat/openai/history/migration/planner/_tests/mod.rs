use super::super::{
    materialize_checkpoint_content, MigrationAccumulator, SourceLocator, SourceNamespace,
};
use super::*;
use domain::FlowRunStatus;
use serde_json::json;

fn fixture() -> (Value, Value, Vec<SourcedReplacement>) {
    let mut state = MigrationAccumulator::new(2).unwrap();
    for item in [
        json!({"role":"user","content":"hello"}),
        json!({"type":"function_call","id":"fc_original","call_id":"c","name":"exec","arguments":"{ \"n\": 9007199254740993 }"}),
    ] {
        state.append(&item).unwrap();
    }
    let old = state.snapshot().old();
    let current = state.snapshot().current();
    let sourced = SourcedReplacement {
        replacement: state.verify(&old).unwrap(),
        source: SourceLocator {
            namespace: SourceNamespace::Native,
            flow_run_id: Uuid::now_v7(),
            node_run_id: Some(Uuid::now_v7()),
            source_id: Uuid::now_v7(),
            sequence: Some(1),
            completion_source_id: Some(Uuid::now_v7()),
            invocation_id: Some("original".into()),
            provider_attempt_index: Some(0),
            response_id: Some("native".into()),
        },
    };
    (old, current, vec![sourced])
}

fn empty() -> ResponsesHistoryFlowSnapshot {
    ResponsesHistoryFlowSnapshot {
        flow: ResponsesHistoryFlowKey {
            scope_id: Uuid::now_v7(),
            application_id: Uuid::now_v7(),
            flow_run_id: Uuid::now_v7(),
        },
        status: FlowRunStatus::WaitingCallback,
        latest_context_version_id: None,
        documents: vec![],
        contexts: vec![],
        checkpoints: vec![],
        continuations: vec![],
    }
}

fn round(proof: &Value) -> Value {
    json!({"response_id":"public","history":proof,"output":[
        {"type":"function_call","id":"fc_original","call_id":"c","name":"exec","arguments":"{ \"n\": 9007199254740993 }"}
    ]})
}

fn callback(proof: &Value) -> Value {
    json!({"provider_metadata":{"native_response":{"response_id":"native","history":proof}},
        "responses_round":round(proof),"history":[{"role":"tool","content":{"history":proof}}],
        "opaque":"{ \"n\": 9007199254740993 }"})
}

fn context(
    sequence: i64,
    parent: Option<Uuid>,
    content: Value,
) -> ResponsesHistoryContextLineageEntry {
    ResponsesHistoryContextLineageEntry {
        identity: ResponsesHistoryContextIdentity {
            context_version_id: Uuid::now_v7(),
            actual_content_id: Uuid::now_v7(),
            content_hash: format!("fixture-{sequence}"),
        },
        previous_projection_id: parent,
        sequence,
        content,
    }
}

fn checkpoint(
    head: Option<&ResponsesHistoryContextLineageEntry>,
    pool: Value,
) -> ResponsesHistoryCheckpoint {
    ResponsesHistoryCheckpoint {
        checkpoint_id: Uuid::now_v7(),
        locator_payload: match head {
            Some(head) => {
                json!({"context_version_id":head.identity.context_version_id,"node_id":"llm"})
            }
            None => json!({"node_id":"llm"}),
        },
        variable_snapshot: pool,
        context: head.map(|head| head.identity.clone()),
    }
}

fn continuation(proof: &Value) -> ResponsesHistoryContinuation {
    ResponsesHistoryContinuation {
        slot_key: "claimed:original".into(),
        encrypted_payload: json!({"cipher":"unchanged-before"}),
        plaintext_digest: "original-digest".into(),
        plaintext_size_bytes: 42,
        key_version: "key-v1".into(),
        hard_expires_at: time::OffsetDateTime::UNIX_EPOCH,
        payload: json!({"native_history":proof,"user":{"native_history":proof},"opaque":"original"}),
    }
}

#[test]
fn only_the_explicit_owned_document_and_capsule_slots_change() {
    let (old, current, verified) = fixture();
    let mut snapshot = empty();
    snapshot.documents = vec![
        ResponsesHistoryDocument {
            owner: ResponsesHistoryDocumentOwner::FlowInput,
            value: json!({"sys":{"responses_input_history":old,"native_inference_recovery":{"history":old}},"user":{"history":old},"native_history":old}),
        },
        ResponsesHistoryDocument {
            owner: ResponsesHistoryDocumentOwner::FlowOutput,
            value: json!({"responses_round":round(&old),"sys":{"responses_input_history":old}}),
        },
        ResponsesHistoryDocument {
            owner: ResponsesHistoryDocumentOwner::CallbackRequest {
                callback_task_id: Uuid::now_v7(),
            },
            value: callback(&old),
        },
        ResponsesHistoryDocument {
            owner: ResponsesHistoryDocumentOwner::NodeOutput {
                node_run_id: Uuid::now_v7(),
            },
            value: callback(&old),
        },
        ResponsesHistoryDocument {
            owner: ResponsesHistoryDocumentOwner::SpanMetadata {
                span_id: Uuid::now_v7(),
            },
            value: json!({"native_response":{"history":old},"user":{"history":old},"responses_round":round(&old)}),
        },
    ];
    snapshot.continuations.push(continuation(&old));
    let before = serde_json::to_value(&snapshot).unwrap();
    let inspection = inspect_flow(&snapshot).unwrap();
    assert_eq!(inspection.proofs, vec![old.clone()]);
    assert_eq!(inspection.public_rounds.len(), 3);
    assert_eq!(
        serde_json::to_value(&inspection.public_rounds[0].output).unwrap(),
        round(&old)["output"]
    );
    let plan = build_plan(&snapshot, &verified).unwrap();
    assert_eq!(plan.documents.len(), 5);
    let expected_paths: &[&[&str]] = &[
        &[
            "/sys/responses_input_history",
            "/sys/native_inference_recovery/history",
        ],
        &["/responses_round/history"],
        &[
            "/provider_metadata/native_response/history",
            "/responses_round/history",
        ],
        &[
            "/provider_metadata/native_response/history",
            "/responses_round/history",
        ],
        &["/native_response/history"],
    ];
    for (index, change) in plan.documents.iter().enumerate() {
        assert_eq!(change.before, snapshot.documents[index].value);
        let mut expected = snapshot.documents[index].value.clone();
        for path in expected_paths[index] {
            *expected.pointer_mut(path).unwrap() = current.clone();
        }
        assert_eq!(change.after, expected, "owner {:?}", change.owner);
    }
    assert_eq!(plan.continuations.len(), 1);
    assert_eq!(plan.continuations[0].before, snapshot.continuations[0]);
    let mut expected = snapshot.continuations[0].payload.clone();
    expected["native_history"] = current;
    assert_eq!(plan.continuations[0].after_payload, expected);
    assert_eq!(
        serde_json::to_value(snapshot).unwrap(),
        before,
        "planning is read-only"
    );
    assert!(plan.contexts.is_empty());
    assert!(plan.checkpoints.is_empty());
}

#[test]
fn null_absent_and_current_proofs_are_preserved_without_receipts() {
    let (_, current, _) = fixture();
    let mut snapshot = empty();
    snapshot.documents.push(ResponsesHistoryDocument {owner:ResponsesHistoryDocumentOwner::FlowInput,
        value:json!({"sys":{"responses_input_history":null,"native_inference_recovery":{"history":current}},"user":{"proof":{"version":999}}})});
    snapshot.documents.push(ResponsesHistoryDocument {owner:ResponsesHistoryDocumentOwner::NodeOutput{node_run_id:Uuid::now_v7()},
        value:json!({"provider_metadata":{"native_response":{"history":null}},"responses_round":{"history":null}})});
    snapshot.continuations.push(continuation(&Value::Null));
    snapshot.checkpoints.push(checkpoint(None, json!({"sys":{"responses_input_history":null},"node":{"__llm_tool_callback":{"provider_metadata":{"native_response":{"history":current}},"responses_round":{"response_id":"not-a-proof"}}}})));
    assert!(collect_proofs(&snapshot).unwrap().is_empty());
    assert!(public_rounds(&snapshot).unwrap().is_empty());
    let plan = build_plan(&snapshot, &[]).unwrap();
    assert!(
        plan.documents.is_empty()
            && plan.contexts.is_empty()
            && plan.checkpoints.is_empty()
            && plan.continuations.is_empty()
    );
}

#[test]
fn unverified_or_malformed_nonnull_owned_proofs_block_planning() {
    let (old, _, verified) = fixture();
    let mut snapshot = empty();
    snapshot.continuations.push(continuation(&old));
    assert!(build_plan(&snapshot, &[]).is_err());
    for malformed in [
        json!({"version":99,"item_count":2,"digest":"0".repeat(64)}),
        json!({"version":2}),
        json!("invalid"),
        json!({"version":2,"item_count":2,"digest":"bad"}),
    ] {
        snapshot.continuations[0].payload["native_history"] = malformed;
        assert!(inspect_flow(&snapshot).is_err());
        assert!(build_plan(&snapshot, &verified).is_err());
    }
}

// Fixture-only lineage expansion. Production planning must preserve each shared
// context's original snapshot/delta encoding and never call this materializer.
fn materialized(contexts: &[ResponsesHistoryContextLineageEntry], head: Uuid) -> Value {
    let index: BTreeMap<_, _> = contexts
        .iter()
        .map(|entry| (entry.identity.context_version_id, entry))
        .collect();
    let mut next = Some(head);
    let mut lineage = Vec::new();
    while let Some(id) = next {
        let entry = index[&id];
        lineage.push(crate::ports::RuntimeContextContentVersion {
            context_version_id: id,
            sequence: entry.sequence,
            content: entry.content.clone(),
        });
        next = entry.previous_projection_id;
    }
    lineage.reverse();
    let mut pool = materialize_checkpoint_content(&lineage).unwrap();
    pool.remove(MARKER);
    Value::Object(pool)
}

fn simulated_copies(
    snapshot: &ResponsesHistoryFlowSnapshot,
    plan: &ResponsesHistoryMigrationPlan,
) -> (
    Vec<ResponsesHistoryContextLineageEntry>,
    BTreeMap<Uuid, Uuid>,
) {
    let mut result = snapshot.contexts.clone();
    let mut remap = BTreeMap::new();
    for (index, change) in plan.contexts.iter().enumerate() {
        let new = Uuid::now_v7();
        let mut entry = change.before.clone();
        entry.identity.context_version_id = new;
        entry.identity.actual_content_id = Uuid::now_v7();
        entry.sequence = 100 + index as i64;
        entry.previous_projection_id = entry
            .previous_projection_id
            .map(|id| remap.get(&id).copied().unwrap_or(id));
        entry.content = change.after_content.clone();
        remap.insert(change.before.identity.context_version_id, new);
        result.push(entry);
    }
    (result, remap)
}

#[test]
fn shared_dag_copies_each_node_once_and_preserves_delta_user_history() {
    let (old, current, verified) = fixture();
    let mut snapshot = empty();
    let root = context(
        1,
        None,
        json!({"format":"runtime_snapshot_v1","variable_pool":{
        "sys":{"responses_input_history":old},"user":{"number":9007199254740993_u64,"lookalike":{"version":999}},
        "llm":{"__llm_tool_callback":callback(&old)},"temporary":"remove-me"}}),
    );
    let a = context(
        2,
        Some(root.identity.context_version_id),
        json!({"format":"runtime_delta_v1","set":{"new_user":{"opaque":"unchanged"}},"remove":["temporary"],
        "llm_callback_appends":[{"node_id":"llm","node_set":{"answer":"kept","provider_metadata":{"native_response":{"history":old}},"responses_round":round(&old)},"node_remove":[],
            "state_set":{"provider_metadata":{"native_response":{"history":old}},"responses_round":round(&old)},"state_remove":[],
            "history_append":[{"user":{"responses_round":{"history":old}},"opaque":"{ x: 1 }"}]}]}),
    );
    let b = context(
        3,
        Some(root.identity.context_version_id),
        json!({"format":"runtime_delta_v1","set":{"branch":"b"},"remove":[]}),
    );
    let overlay = context(
        4,
        Some(a.identity.context_version_id),
        json!({"format":"runtime_delta_v1","set":{},"remove":[],
        "llm_callback_appends":[{"node_id":"llm","node_overlay":{"overlay_value":"kept","provider_metadata":{"native_response":{"history":old}},"responses_round":round(&old)},
            "state_overlay":{"provider_metadata":{"native_response":{"history":old}},"responses_round":round(&old)},
            "system_changed":false,"history_append":[{"history":old,"opaque":"raw appended"}]}]}),
    );
    // A later independent context is unchanged but must remain repository latest.
    let latest = context(
        9,
        None,
        json!({"format":"runtime_snapshot_v1","variable_pool":{"latest_user":"unchanged"}}),
    );
    for head in [&a, &a, &b, &overlay, &latest] {
        snapshot.checkpoints.push(checkpoint(
            Some(head),
            json!({MARKER:{"context_version_id":head.identity.context_version_id}}),
        ));
    }
    snapshot.latest_context_version_id = Some(latest.identity.context_version_id);
    snapshot.contexts = vec![root, a, b, overlay, latest];
    let plan = build_plan(&snapshot, &verified).unwrap();
    assert_eq!(
        plan.contexts.len(),
        5,
        "shared nodes copied once, independent of checkpoint count"
    );
    let unique: BTreeSet<_> = plan
        .contexts
        .iter()
        .map(|change| change.before.identity.context_version_id)
        .collect();
    assert_eq!(unique.len(), 5);
    assert_eq!(
        plan.contexts
            .last()
            .unwrap()
            .before
            .identity
            .context_version_id,
        snapshot.latest_context_version_id.unwrap()
    );
    assert_eq!(
        plan.contexts.last().unwrap().after_content,
        snapshot.contexts[4].content,
        "latest anchor content stays unchanged"
    );
    assert_eq!(
        plan.contexts[2].after_content, snapshot.contexts[2].content,
        "unchanged branch child retains delta encoding"
    );
    assert!(plan
        .checkpoints
        .iter()
        .all(|change| change.legacy_inline_variable_pool.is_none()));
    assert_eq!(plan.checkpoints.len(), 5);
    let (copies, remap) = simulated_copies(&snapshot, &plan);
    for checkpoint in &snapshot.checkpoints {
        let head = checkpoint.context.as_ref().unwrap().context_version_id;
        let mut expected = materialized(&snapshot.contexts, head);
        if let Some(proof) = expected.pointer_mut("/sys/responses_input_history") {
            *proof = current.clone();
        }
        if let Some(proof) = expected
            .pointer_mut("/llm/__llm_tool_callback/provider_metadata/native_response/history")
        {
            *proof = current.clone();
        }
        if let Some(proof) =
            expected.pointer_mut("/llm/__llm_tool_callback/responses_round/history")
        {
            *proof = current.clone();
        }
        for path in [
            "/llm/provider_metadata/native_response/history",
            "/llm/responses_round/history",
        ] {
            if let Some(proof) = expected.pointer_mut(path) {
                *proof = current.clone();
            }
        }
        assert_eq!(materialized(&copies, remap[&head]), expected);
    }
    assert_eq!(
        plan.contexts[1].after_content["llm_callback_appends"][0]["history_append"],
        snapshot.contexts[1].content["llm_callback_appends"][0]["history_append"]
    );
    assert_eq!(
        plan.contexts[3].after_content["llm_callback_appends"][0]["history_append"],
        snapshot.contexts[3].content["llm_callback_appends"][0]["history_append"]
    );
    assert_eq!(collect_proofs(&snapshot).unwrap(), vec![old]);
    assert_eq!(
        public_rounds(&snapshot).unwrap().len(),
        5,
        "source rounds read once per unique context"
    );
}

#[test]
fn inline_checkpoint_changes_preserve_user_pool_and_anchor_latest_existing_context() {
    let (old, current, verified) = fixture();
    let mut snapshot = empty();
    let latest = context(
        7,
        None,
        json!({"format":"runtime_snapshot_v1","variable_pool":{"latest":"kept"}}),
    );
    snapshot.latest_context_version_id = Some(latest.identity.context_version_id);
    snapshot.contexts.push(latest.clone());
    let pool = json!({"sys":{"responses_input_history":old},"llm":{"__llm_tool_callback":callback(&old)},"user":{"history":old},MARKER:{"sequence":2}});
    snapshot.checkpoints.push(checkpoint(None, pool.clone()));
    let plan = build_plan(&snapshot, &verified).unwrap();
    assert_eq!(plan.contexts.len(), 1);
    assert_eq!(plan.contexts[0].before, latest);
    assert_eq!(plan.contexts[0].after_content, latest.content);
    let after = plan.checkpoints[0]
        .legacy_inline_variable_pool
        .as_ref()
        .unwrap();
    let mut expected = pool;
    expected["sys"]["responses_input_history"] = current.clone();
    expected["llm"]["__llm_tool_callback"]["provider_metadata"]["native_response"]["history"] =
        current.clone();
    expected["llm"]["__llm_tool_callback"]["responses_round"]["history"] = current;
    assert_eq!(after, &expected);
    assert!(plan.checkpoints[0].before_context.is_none());
}

#[test]
fn no_existing_latest_includes_last_inline_checkpoint_unchanged_as_anchor() {
    let (old, _, verified) = fixture();
    let mut snapshot = empty();
    snapshot.checkpoints.push(checkpoint(
        None,
        json!({"sys":{"responses_input_history":old},"user":"first"}),
    ));
    snapshot
        .checkpoints
        .push(checkpoint(None, json!({"user":{"opaque":"final state"}})));
    let plan = build_plan(&snapshot, &verified).unwrap();
    assert!(plan.contexts.is_empty());
    assert_eq!(plan.checkpoints.len(), 2);
    assert_eq!(
        plan.checkpoints.last().unwrap().checkpoint_id,
        snapshot.checkpoints[1].checkpoint_id
    );
    assert_eq!(
        plan.checkpoints[1].legacy_inline_variable_pool.as_ref(),
        Some(&snapshot.checkpoints[1].variable_snapshot)
    );
}

#[test]
fn checkpoints_cannot_hide_missing_or_mismatched_context_data() {
    let mut snapshot = empty();
    let id = Uuid::now_v7();
    snapshot.checkpoints.push(ResponsesHistoryCheckpoint {
        checkpoint_id: Uuid::now_v7(),
        locator_payload: json!({"context_version_id":id}),
        variable_snapshot: json!({MARKER:{"context_version_id":id}}),
        context: None,
    });
    assert!(inspect_flow(&snapshot).is_err());
    assert!(build_plan(&snapshot, &[]).is_err());
    snapshot.checkpoints[0].locator_payload = json!({});
    assert!(
        inspect_flow(&snapshot).is_err(),
        "marker-only data is not a user variable pool"
    );
}

#[test]
fn malformed_owned_delta_fields_block_but_nested_user_lookalikes_do_not() {
    let (old, _, verified) = fixture();
    let mut snapshot = empty();
    let root = context(
        1,
        None,
        json!({"format":"runtime_snapshot_v1","variable_pool":{"llm":{"__llm_tool_callback":{"history":[]}}}}),
    );
    let delta = context(
        2,
        Some(root.identity.context_version_id),
        json!({"format":"runtime_delta_v1","set":{"user":{"sys":{"responses_input_history":{"version":999}}}},"remove":[],
        "llm_callback_appends":[{"node_id":"llm","node_set":{},"node_remove":[],"state_set":{"provider_metadata":{"native_response":{"history":old}}},"state_remove":[],"history_append":[{"native_history":{"version":999}}]}]}),
    );
    snapshot.latest_context_version_id = Some(delta.identity.context_version_id);
    snapshot.contexts = vec![root, delta];
    assert_eq!(collect_proofs(&snapshot).unwrap(), vec![old]);
    assert!(build_plan(&snapshot, &verified).is_ok());
    snapshot.contexts[1].content["llm_callback_appends"][0]["state_set"]["provider_metadata"]
        ["native_response"]["history"] = json!({"version":999});
    assert!(inspect_flow(&snapshot).is_err());
    assert!(build_plan(&snapshot, &verified).is_err());
}

#[test]
fn current_context_and_null_delta_slots_do_not_create_versions_for_markers() {
    let (_, current, _) = fixture();
    let mut snapshot = empty();
    let root = context(
        1,
        None,
        json!({"format":"runtime_snapshot_v1","variable_pool":{"sys":{"responses_input_history":current},"llm":{"__llm_tool_callback":{"history":[],"provider_metadata":{"native_response":{"history":null}}}}}}),
    );
    let delta = context(
        2,
        Some(root.identity.context_version_id),
        json!({"format":"runtime_delta_v1","set":{"sys":{"native_inference_recovery":{"history":null}}},"remove":[],"llm_callback_appends":[{"node_id":"llm","node_set":{},"node_remove":[],"state_set":{"provider_metadata":{"native_response":{"history":null}}},"state_remove":[],"history_append":[]}]}),
    );
    snapshot
        .checkpoints
        .push(checkpoint(Some(&delta), json!({MARKER:{"sequence":2}})));
    snapshot.latest_context_version_id = Some(delta.identity.context_version_id);
    snapshot.contexts = vec![root, delta];
    assert!(collect_proofs(&snapshot).unwrap().is_empty());
    let plan = build_plan(&snapshot, &[]).unwrap();
    assert!(plan.contexts.is_empty());
    assert!(plan.checkpoints.is_empty());
}

#[test]
fn public_round_collection_retains_original_order_numbers_and_opaque_fields() {
    let mut snapshot = empty();
    let output = vec![
        json!({"type":"reasoning","encrypted_content":"opaque","number":9007199254740993_u64}),
        json!({"type":"function_call","call_id":"z","name":"z_tool","arguments":"{  \"n\": 1 }"}),
        json!({"type":"function_call","call_id":"a","name":"a_tool","arguments":"{}"}),
    ];
    let mut state = MigrationAccumulator::new(2).unwrap();
    for item in &output {
        state.append(item).unwrap();
    }
    snapshot.documents.push(ResponsesHistoryDocument {owner:ResponsesHistoryDocumentOwner::FlowOutput,
        value:json!({"responses_round":{"response_id":"ordered","history":state.snapshot().old(),"output":output}})});
    assert_eq!(public_rounds(&snapshot).unwrap()[0].output, output);
}

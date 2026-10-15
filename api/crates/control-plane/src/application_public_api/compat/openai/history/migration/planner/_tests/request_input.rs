use super::*;
use crate::responses_history_migration::{
    HistorySourceGraph, ScopedHistoryTarget, SourceCoverage, SourceGraphResolution,
};

fn raw_request(old: &Value) -> Value {
    let call = round(old)["output"][0].clone();
    json!({"frame_kind":"request","encoding":"utf8","body":json!({"input":[{"role":"user","content":"hello"},call]}).to_string()})
}

fn resolve_input(snapshot: &ResponsesHistoryFlowSnapshot, old: &Value) -> SourceGraphResolution {
    let inspection = inspect_flow(snapshot).unwrap();
    let scoped = |proof: Value| ScopedHistoryTarget {
        flow_run_id: snapshot.flow.flow_run_id,
        proof,
    };
    let full: Vec<_> = inspection
        .full_source_required_proofs
        .into_iter()
        .map(scoped)
        .collect();
    let input: Vec<_> = inspection
        .request_input_histories
        .into_iter()
        .map(scoped)
        .collect();
    let mut graph = HistorySourceGraph::with_request_input_targets(&full, &input).unwrap();
    graph
        .push_public_request_input(
            snapshot.flow.flow_run_id,
            Uuid::now_v7(),
            17,
            &raw_request(old),
        )
        .unwrap();
    graph.resolve().unwrap()
}

fn input_snapshot(old: &Value) -> ResponsesHistoryFlowSnapshot {
    let mut snapshot = empty();
    snapshot.documents.push(ResponsesHistoryDocument {
        owner: ResponsesHistoryDocumentOwner::FlowInput,
        value: json!({"sys":{"responses_input_history":old},"user":{"history":old}}),
    });
    snapshot
}

#[test]
fn input_receipt_updates_only_host_input_slots_and_checkpoint_sys_copies() {
    let (old, current, _) = fixture();
    let mut snapshot = input_snapshot(&old);
    let root = context(
        1,
        None,
        json!({"format":"runtime_snapshot_v1","variable_pool":{"sys":{"responses_input_history":old},
        "user":{"integer":9007199254740993_u64,"history":old}}}),
    );
    let delta = context(
        2,
        Some(root.identity.context_version_id),
        json!({"format":"runtime_delta_v1","set":{"sys":{"responses_input_history":old},"user_delta":"opaque"},"remove":[]}),
    );
    snapshot.checkpoints.push(checkpoint(
        Some(&delta),
        json!({MARKER:{"context_version_id":delta.identity.context_version_id}}),
    ));
    snapshot.checkpoints.push(checkpoint(
        None,
        json!({"sys":{"responses_input_history":old},"user":{"history":old}}),
    ));
    snapshot.latest_context_version_id = Some(delta.identity.context_version_id);
    snapshot.contexts = vec![root, delta];
    let inspection = inspect_flow(&snapshot).unwrap();
    assert_eq!(inspection.proofs, vec![old.clone()]);
    assert_eq!(inspection.request_input_histories, vec![old.clone()]);
    assert!(inspection.full_source_required_proofs.is_empty());
    let resolution = resolve_input(&snapshot, &old);
    assert!(resolution.unresolved_targets.is_empty());
    assert_eq!(
        resolution.verified[0].coverage(),
        SourceCoverage::RequestInput
    );
    let plan = build_plan(&snapshot, &resolution.verified).unwrap();
    assert_eq!(plan.documents.len(), 1);
    assert_eq!(
        plan.documents[0].after["sys"]["responses_input_history"],
        current
    );
    assert_eq!(plan.documents[0].after["user"]["history"], old);
    assert_eq!(plan.contexts.len(), 2);
    assert_eq!(
        plan.contexts[0].after_content["variable_pool"]["sys"]["responses_input_history"],
        current
    );
    assert_eq!(
        plan.contexts[0].after_content["variable_pool"]["user"],
        snapshot.contexts[0].content["variable_pool"]["user"]
    );
    assert_eq!(
        plan.contexts[1].after_content["set"]["sys"]["responses_input_history"],
        current
    );
    let inline = plan
        .checkpoints
        .iter()
        .find_map(|change| change.legacy_inline_variable_pool.as_ref())
        .unwrap();
    assert_eq!(inline["sys"]["responses_input_history"], current);
    assert_eq!(inline["user"]["history"], old);
    let mut foreign = snapshot;
    foreign.flow.flow_run_id = Uuid::from_u128(2);
    assert!(
        build_plan(&foreign, &resolution.verified).is_err(),
        "an input receipt cannot be relabelled for another flow"
    );
}

#[test]
fn same_hash_in_any_noninput_owned_slot_still_requires_full_source() {
    let (old, _, _) = fixture();
    let base = input_snapshot(&old);
    let mut cases = Vec::new();
    let mut recovery = base.clone();
    recovery.documents[0].value["sys"]["native_inference_recovery"] = json!({"history":old});
    cases.push(recovery);
    for (owner, value) in [
        (
            ResponsesHistoryDocumentOwner::FlowOutput,
            json!({"responses_round":round(&old)}),
        ),
        (
            ResponsesHistoryDocumentOwner::CallbackRequest {
                callback_task_id: Uuid::now_v7(),
            },
            json!({"provider_metadata":{"native_response":{"history":old}}}),
        ),
        (
            ResponsesHistoryDocumentOwner::NodeOutput {
                node_run_id: Uuid::now_v7(),
            },
            json!({"responses_round":round(&old)}),
        ),
        (
            ResponsesHistoryDocumentOwner::SpanMetadata {
                span_id: Uuid::now_v7(),
            },
            json!({"native_response":{"history":old}}),
        ),
    ] {
        let mut snapshot = base.clone();
        snapshot
            .documents
            .push(ResponsesHistoryDocument { owner, value });
        cases.push(snapshot);
    }
    let mut capsule = base.clone();
    capsule.continuations.push(continuation(&old));
    cases.push(capsule);
    for pool in [
        json!({"sys":{"native_inference_recovery":{"history":old}}}),
        json!({"llm":{"__llm_tool_callback":{"provider_metadata":{"native_response":{"history":old}}}}}),
        json!({"llm":{"__llm_tool_callback":{"responses_round":round(&old)}}}),
    ] {
        let mut snapshot = base.clone();
        let root = context(
            1,
            None,
            json!({"format":"runtime_snapshot_v1","variable_pool":pool}),
        );
        snapshot.checkpoints.push(checkpoint(
            Some(&root),
            json!({MARKER:{"context_version_id":root.identity.context_version_id}}),
        ));
        snapshot.latest_context_version_id = Some(root.identity.context_version_id);
        snapshot.contexts.push(root);
        cases.push(snapshot);
    }
    for snapshot in cases {
        let inspection = inspect_flow(&snapshot).unwrap();
        assert_eq!(inspection.request_input_histories, vec![old.clone()]);
        assert_eq!(inspection.full_source_required_proofs, vec![old.clone()]);
        let resolution = resolve_input(&snapshot, &old);
        assert_eq!(resolution.verified.len(), 1);
        assert_eq!(
            resolution.verified[0].coverage(),
            SourceCoverage::RequestInput
        );
        assert_eq!(
            resolution.unresolved_targets,
            vec![ScopedHistoryTarget {
                flow_run_id: snapshot.flow.flow_run_id,
                proof: old.clone()
            }]
        );
        let error = build_plan(&snapshot, &resolution.verified).unwrap_err();
        assert!(
            error.to_string().contains("full_source_required"),
            "same hash never widens receipt coverage: {error}"
        );
    }
}

#[test]
fn inspection_classifies_actual_occurrences_not_hashes_or_opaque_shapes() {
    let (old, current, _) = fixture();
    let mut snapshot = input_snapshot(&old);
    snapshot.documents[0].value["sys"]["native_inference_recovery"] = json!({"history":old});
    snapshot.checkpoints.push(checkpoint(
        None,
        json!({"sys":{"responses_input_history":current},
        "user":{"sys":{"responses_input_history":{"version":999}}}}),
    ));
    let provider = context(
        9,
        None,
        json!({"format":"runtime_snapshot_v1","variable_pool":{"sys":{"responses_input_history":{"version":999}}}}),
    );
    snapshot.latest_context_version_id = Some(provider.identity.context_version_id);
    snapshot.contexts.push(provider);
    let inspection = inspect_flow(&snapshot).unwrap();
    assert_eq!(inspection.proofs, vec![old.clone(), current.clone()]);
    assert_eq!(
        inspection.request_input_histories,
        vec![old.clone(), current]
    );
    assert_eq!(inspection.full_source_required_proofs, vec![old]);
}

#[test]
fn full_receipt_still_covers_request_and_noninput_slots() {
    let (old, current, full) = fixture();
    let mut snapshot = input_snapshot(&old);
    snapshot.continuations.push(continuation(&old));
    let plan = build_plan(&snapshot, &full).unwrap();
    assert_eq!(
        plan.documents[0].after["sys"]["responses_input_history"],
        current
    );
    assert_eq!(
        plan.continuations[0].after_payload["native_history"],
        current
    );
}

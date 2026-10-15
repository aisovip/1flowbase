use super::*;
use serde_json::json;

fn proof(items: &[Value]) -> Value {
    let mut state = MigrationAccumulator::new(2).unwrap();
    for item in items {
        state.append(item).unwrap();
    }
    state.snapshot().old()
}

fn current(items: &[Value]) -> Value {
    let mut state = MigrationAccumulator::new(2).unwrap();
    for item in items {
        state.append(item).unwrap();
    }
    state.snapshot().current()
}

fn prepared(body: Value, invocation: Uuid, attempt: u64) -> Value {
    json!({"kind":"model_call","direction":"prepared","status":"recorded",
        "invocation_id":invocation,"provider_attempt_index":attempt,
        "body":json!({"native_request":{"wire_body":body},"messages":[{"role":"user","content":"WRONG preview"}]}).to_string()})
}

fn received(response: &str, invocation: Uuid, attempt: u64) -> Value {
    // Tool calls are absent from the observer's output_items on purpose.
    json!({"kind":"model_reply","direction":"received","status":"recorded",
        "invocation_id":invocation,"provider_attempt_index":attempt,
        "body":json!({"output_items":[],"result":{"response_id":response,"finish_reason":"tool_call"}}).to_string()})
}

fn feed_round(
    graph: &mut HistorySourceGraph,
    flow: Uuid,
    node: Uuid,
    body: Value,
    output: &[(u64, Value)],
    response: &str,
) {
    let invocation = Uuid::now_v7();
    graph
        .push_native(
            flow,
            Some(node),
            Uuid::now_v7(),
            1,
            "provider_semantic_step",
            &prepared(body, invocation, 0),
        )
        .unwrap();
    for (position, (index, item)) in output.iter().enumerate() {
        graph
            .push_native(
                flow,
                Some(node),
                Uuid::now_v7(),
                position as i64 + 2,
                "provider_output_item_done",
                &json!({"output_index":index,"item":item}),
            )
            .unwrap();
    }
    graph
        .push_native(
            flow,
            Some(node),
            Uuid::now_v7(),
            output.len() as i64 + 2,
            "provider_semantic_step",
            &received(response, invocation, 0),
        )
        .unwrap();
    graph.finish_native_flow(flow).unwrap();
}

fn frame(body: &Value) -> Value {
    json!({"encoding":"utf8","frame_kind":"request","body":body.to_string()})
}

fn round(response: &str, all_items: &[Value], output: Vec<Value>) -> ResponsesRoundEvidence {
    ResponsesRoundEvidence {
        response_id: response.into(),
        history: proof(all_items),
        output,
    }
}

#[test]
fn native_indexed_tools_are_complete_and_follow_provider_order() {
    let input = json!({"role":"user","content":"work"});
    let first = json!({"type":"function_call","id":"fc_z","call_id":"z","name":"z_tool","arguments":"{ \"n\": 1 }"});
    let second =
        json!({"type":"function_call","id":"fc_a","call_id":"a","name":"a_tool","arguments":"{}"});
    let items = vec![input.clone(), first.clone(), second.clone()];
    let expected = proof(&items);
    let mut graph = HistorySourceGraph::new(&[expected.clone()]).unwrap();
    feed_round(
        &mut graph,
        Uuid::now_v7(),
        Uuid::now_v7(),
        json!({"input":[input]}),
        &[(17, second), (3, first)],
        "native_one",
    );
    assert!(graph.active_native.is_empty());
    assert!(
        graph.pending.is_empty(),
        "resolved raw body/output must be released"
    );
    assert_eq!(graph.responses.len(), 1);
    let result = graph.resolve().unwrap();
    assert!(result.unresolved_targets.is_empty());
    assert!(result.unresolved_sources.is_empty());
    assert_eq!(result.verified[0].replacement.old(), expected);
    assert_eq!(result.verified[0].replacement.current(), current(&items));
    assert_eq!(result.verified[0].source.namespace, SourceNamespace::Native);
    assert!(result.verified[0].source.completion_source_id.is_some());
}

#[test]
fn forward_predecessors_are_retained_then_resolved_across_flows() {
    let input = json!({"role":"user","content":"start"});
    let call =
        json!({"type":"function_call","id":"fc_a","call_id":"a","name":"exec","arguments":"{}"});
    let result_item = json!({"type":"function_call_output","call_id":"a","output":"opaque result"});
    let all = vec![input.clone(), call.clone(), result_item.clone()];
    let mut graph = HistorySourceGraph::new(&[proof(&all)]).unwrap();
    feed_round(
        &mut graph,
        Uuid::now_v7(),
        Uuid::now_v7(),
        json!({"previous_response_id":"first","input":[result_item]}),
        &[],
        "second",
    );
    assert_eq!(graph.pending.len(), 1);
    assert!(graph.verified.is_empty());
    feed_round(
        &mut graph,
        Uuid::now_v7(),
        Uuid::now_v7(),
        json!({"input":[input]}),
        &[(0, call)],
        "first",
    );
    assert!(graph.pending.is_empty());
    let result = graph.resolve().unwrap();
    assert_eq!(result.verified.len(), 1);
    assert!(result.unresolved_targets.is_empty());
    assert!(result.unresolved_sources.is_empty());
}

#[test]
fn missing_predecessors_and_conflicting_output_indexes_never_publish_receipts() {
    let input = json!({"role":"user","content":"work"});
    let output = json!({"type":"reasoning","encrypted_content":"cipher","summary":[]});
    let expected = proof(&[input.clone(), output.clone()]);
    let mut missing = HistorySourceGraph::new(&[expected.clone()]).unwrap();
    feed_round(
        &mut missing,
        Uuid::now_v7(),
        Uuid::now_v7(),
        json!({"previous_response_id":"unavailable","input":[input]}),
        &[(0, output.clone())],
        "missing",
    );
    let result = missing.resolve().unwrap();
    assert!(result.verified.is_empty());
    assert_eq!(result.unresolved_targets, vec![expected.clone()]);
    assert!(result.unresolved_sources[0]
        .reason
        .contains("predecessor_unresolved"));
    let mut conflict = HistorySourceGraph::new(&[expected.clone()]).unwrap();
    feed_round(
        &mut conflict,
        Uuid::now_v7(),
        Uuid::now_v7(),
        json!({"input":[input]}),
        &[
            (0, output),
            (0, json!({"type":"reasoning","encrypted_content":"CHANGED"})),
        ],
        "conflict",
    );
    let result = conflict.resolve().unwrap();
    assert!(result.verified.is_empty());
    assert_eq!(result.unresolved_targets, vec![expected]);
    assert!(result.unresolved_sources[0]
        .reason
        .contains("output_index_conflict"));
}

#[test]
fn native_requires_original_wire_body_and_matching_attempt() {
    let item = json!({"role":"user","content":"work"});
    let target = proof(std::slice::from_ref(&item));
    for invalid_attempt in [false, true] {
        let mut graph = HistorySourceGraph::new(&[target.clone()]).unwrap();
        let flow = Uuid::now_v7();
        let node = Uuid::now_v7();
        let invocation = Uuid::now_v7();
        let mut start = prepared(json!({"input":[item]}), invocation, 1);
        if !invalid_attempt {
            start["body"] = json!(json!({"messages":[item],"preview":"work"}).to_string());
        }
        graph
            .push_native(
                flow,
                Some(node),
                Uuid::now_v7(),
                1,
                "provider_semantic_step",
                &start,
            )
            .unwrap();
        graph
            .push_native(
                flow,
                Some(node),
                Uuid::now_v7(),
                2,
                "provider_semantic_step",
                &received("response", invocation, if invalid_attempt { 2 } else { 1 }),
            )
            .unwrap();
        let result = graph.resolve().unwrap();
        assert!(result.verified.is_empty());
        assert_eq!(result.unresolved_targets, vec![target.clone()]);
        assert_eq!(result.unresolved_sources.len(), 1);
    }
}

#[test]
fn original_json_numbers_and_opaque_arguments_never_cross_node_reserialization() {
    let body: Value = serde_json::from_str(r#"{"input":[{"type":"future_item","integer":9007199254740993,"decimal":1.0,"opaque":"雪"},{"type":"function_call","id":"fc_1","call_id":"c","name":"tool","arguments":"{ \"n\": 9007199254740993 }"}]}"#).unwrap();
    let items = body["input"].as_array().unwrap();
    let expected = proof(items);
    let mut graph = HistorySourceGraph::new(&[expected.clone()]).unwrap();
    feed_round(
        &mut graph,
        Uuid::now_v7(),
        Uuid::now_v7(),
        body.clone(),
        &[],
        "exact",
    );
    let result = graph.resolve().unwrap();
    assert_eq!(result.verified[0].replacement.old(), expected);
    assert_eq!(result.verified[0].replacement.current(), current(items));
    let mut changed = body;
    changed["input"][0]["integer"] = json!(9007199254740992_u64);
    let mut graph = HistorySourceGraph::new(&[expected]).unwrap();
    feed_round(
        &mut graph,
        Uuid::now_v7(),
        Uuid::now_v7(),
        changed,
        &[],
        "rounded",
    );
    assert!(graph.resolve().unwrap().verified.is_empty());
}

#[test]
fn public_original_requests_and_round_output_verify_every_full_input_prefix() {
    let first = json!({"role":"user","content":"first"});
    let second = json!({"role":"user","content":"second"});
    let output = json!({"type":"message","role":"assistant","content":"answer"});
    let all = vec![first.clone(), second.clone(), output.clone()];
    let targets = vec![proof(std::slice::from_ref(&first)), proof(&all)];
    let evidence = round("public_one", &all, vec![output]);
    for encoding in ["utf8", "base64"] {
        let mut original = frame(&json!({"input":[first,second]}));
        if encoding == "base64" {
            original["body"] = json!(STANDARD.encode(original["body"].as_str().unwrap()));
            original["encoding"] = json!("base64");
        }
        let mut graph = HistorySourceGraph::new(&targets).unwrap();
        graph
            .push_public(Uuid::now_v7(), Uuid::now_v7(), &original, &evidence)
            .unwrap();
        assert!(graph.pending.is_empty());
        let result = graph.resolve().unwrap();
        assert_eq!(result.verified.len(), 2);
        assert!(result.unresolved_targets.is_empty());
        assert!(result
            .verified
            .iter()
            .all(|entry| entry.source.namespace == SourceNamespace::Public));
    }
}

#[test]
fn public_wrong_input_or_wrong_round_discards_even_matching_prefixes() {
    let input = json!({"role":"user","content":"hello"});
    let output = json!({"type":"message","role":"assistant","content":"answer"});
    let all = vec![input.clone(), output.clone()];
    for tamper_round in [false, true] {
        let mut body = json!({"input":[input]});
        let mut evidence = round("public_one", &all, vec![output.clone()]);
        if tamper_round {
            evidence.output[0]["content"] = json!("changed");
        } else {
            body["input"][0]["content"] = json!("changed");
        }
        let mut graph =
            HistorySourceGraph::new(&[proof(&all), proof(std::slice::from_ref(&input))]).unwrap();
        graph
            .push_public(Uuid::now_v7(), Uuid::now_v7(), &frame(&body), &evidence)
            .unwrap();
        let result = graph.resolve().unwrap();
        assert!(result.verified.is_empty());
        assert_eq!(result.unresolved_targets.len(), 2);
        assert_eq!(result.unresolved_sources.len(), 1);
    }
}

#[test]
fn public_cursor_dependencies_resolve_forward_and_cannot_use_native_response_names() {
    let input = json!({"role":"user","content":"hello"});
    let delta = json!({"role":"user","content":"next"});
    let all = vec![input.clone(), delta.clone()];
    let mut graph = HistorySourceGraph::new(&[proof(&all)]).unwrap();
    graph
        .push_public(
            Uuid::now_v7(),
            Uuid::now_v7(),
            &frame(&json!({"previous_response_id":"shared","input":[delta]})),
            &round("public_next", &all, vec![]),
        )
        .unwrap();
    feed_round(
        &mut graph,
        Uuid::now_v7(),
        Uuid::now_v7(),
        json!({"input":[input]}),
        &[],
        "shared",
    );
    assert_eq!(
        graph.pending.len(),
        1,
        "native response cannot seed public history"
    );
    graph
        .push_public(
            Uuid::now_v7(),
            Uuid::now_v7(),
            &frame(&json!({"input":[input]})),
            &round("shared", std::slice::from_ref(&input), vec![]),
        )
        .unwrap();
    let result = graph.resolve().unwrap();
    assert!(result.unresolved_targets.is_empty());
    assert_eq!(result.verified.len(), 1);
}

#[test]
fn targets_reject_unknown_formats_and_current_format_is_skipped() {
    let item = json!({"role":"user","content":"hello"});
    assert!(
        HistorySourceGraph::new(&[current(std::slice::from_ref(&item))])
            .unwrap()
            .resolve()
            .unwrap()
            .verified
            .is_empty()
    );
    let legacy = proof(&[item]);
    for version in [json!(0), json!(4), json!("2"), json!(2.0), Value::Null] {
        let mut target = legacy.clone();
        target["version"] = version;
        assert!(HistorySourceGraph::new(&[target]).is_err());
    }
}

#[test]
fn checkpoint_materializer_preserves_user_pool_and_recovery_marker() {
    let id = Uuid::now_v7();
    let lineage = [crate::ports::RuntimeContextContentVersion {
        context_version_id: id,
        sequence: 7,
        content: json!({"format":"runtime_snapshot_v1","variable_pool":{
            "user":{"number":9007199254740993_u64,"opaque":"cipher"},"retained":true}}),
    }];
    let pool = super::super::materialize_checkpoint_content(&lineage).unwrap();
    assert_eq!(pool["user"]["number"], json!(9007199254740993_u64));
    assert_eq!(pool["user"]["opaque"], "cipher");
    assert_eq!(pool["retained"], true);
    assert_eq!(
        pool["__runtime_recovery_context"]["context_version_id"],
        json!(id)
    );
    assert_eq!(pool["__runtime_recovery_context"]["sequence"], 7);
}

#[test]
fn next_prepared_closes_only_the_previous_invocation_window() {
    let first = json!({"role":"user","content":"first"});
    let second = json!({"role":"user","content":"second"});
    let mut graph = HistorySourceGraph::new(&[
        proof(std::slice::from_ref(&first)),
        proof(std::slice::from_ref(&second)),
    ])
    .unwrap();
    let flow = Uuid::now_v7();
    let node = Uuid::now_v7();
    let a = Uuid::now_v7();
    let b = Uuid::now_v7();
    graph
        .push_native(
            flow,
            Some(node),
            Uuid::now_v7(),
            1,
            "provider_semantic_step",
            &prepared(json!({"input":[first]}), a, 0),
        )
        .unwrap();
    graph
        .push_native(
            flow,
            Some(node),
            Uuid::now_v7(),
            2,
            "provider_semantic_step",
            &received("first", a, 0),
        )
        .unwrap();
    graph
        .push_native(
            flow,
            Some(node),
            Uuid::now_v7(),
            3,
            "provider_semantic_step",
            &prepared(json!({"input":[second]}), b, 1),
        )
        .unwrap();
    assert_eq!(graph.responses.len(), 1);
    assert!(graph.pending.is_empty());
    graph
        .push_native(
            flow,
            Some(node),
            Uuid::now_v7(),
            4,
            "provider_semantic_step",
            &received("second", b, 1),
        )
        .unwrap();
    let result = graph.resolve().unwrap();
    assert_eq!(result.verified.len(), 2);
    assert!(result.unresolved_targets.is_empty());
}

#[test]
fn conflicting_source_replacements_poison_the_graph() {
    let first = json!({"type":"future_item","id":"first"});
    let second = json!({"type":"future_item","id":"second"});
    let mut state = MigrationAccumulator::new(1).unwrap();
    state.append(&first).unwrap();
    let target = state.snapshot().old();
    let mut graph = HistorySourceGraph::new(&[target.clone()]).unwrap();
    graph
        .push_public(
            Uuid::now_v7(),
            Uuid::now_v7(),
            &frame(&json!({"input":[first]})),
            &ResponsesRoundEvidence {
                response_id: "one".into(),
                history: target.clone(),
                output: vec![],
            },
        )
        .unwrap();
    // Frozen v1 drops both unprefixed IDs, while the current extension contract
    // preserves them. Two authentic sources would imply different replacements.
    assert!(graph
        .push_public(
            Uuid::now_v7(),
            Uuid::now_v7(),
            &frame(&json!({"input":[second]})),
            &ResponsesRoundEvidence {
                response_id: "two".into(),
                history: target,
                output: vec![]
            }
        )
        .is_err());
    assert!(
        graph.resolve().is_err(),
        "a poisoned graph cannot return write receipts"
    );
}

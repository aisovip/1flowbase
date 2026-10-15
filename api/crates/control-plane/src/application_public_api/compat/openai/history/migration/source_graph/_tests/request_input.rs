use super::*;

fn input_graph(proofs: &[Value]) -> HistorySourceGraph {
    HistorySourceGraph::with_request_input_targets(
        &[],
        &proofs.iter().cloned().map(target).collect::<Vec<_>>(),
    )
    .unwrap()
}

#[test]
fn entire_original_input_attests_only_input_with_exact_request_locator() {
    let body:Value=serde_json::from_str(r#"{"input":[{"type":"future","integer":9007199254740993,"decimal":1.0,"opaque":{"unknown":["雪"]}},{"type":"function_call","id":"fc_1","call_id":"c","name":"tool","arguments":"{ \"n\": 9007199254740993 }"}],"extension":{"unknown":true},"response_id":"not-an-alias","output":[{"type":"future","opaque":"not-evidence"}]}"#).unwrap();
    let items = body["input"].as_array().unwrap();
    let expected = proof(items);
    for encoding in ["utf8", "base64"] {
        let mut original = frame(&body);
        original["sequence"] = json!(37);
        if encoding == "base64" {
            original["body"] = json!(STANDARD.encode(original["body"].as_str().unwrap()));
            original["encoding"] = json!("base64");
        }
        let request = Uuid::now_v7();
        let mut graph = input_graph(&[expected.clone()]);
        graph
            .push_public_request_input(test_flow(), request, 37, &original)
            .unwrap();
        assert!(
            graph.responses.is_empty(),
            "an input attestation cannot create a response alias"
        );
        let result = graph.resolve().unwrap();
        assert!(result.unresolved_targets.is_empty());
        assert_eq!(result.verified.len(), 1);
        let receipt = &result.verified[0];
        assert_eq!(receipt.coverage(), SourceCoverage::RequestInput);
        assert_eq!(receipt.replacement().old(), expected);
        assert_eq!(receipt.replacement().current(), current(items));
        assert_eq!(receipt.source().flow_run_id, test_flow());
        assert_eq!(receipt.source().source_id, request);
        assert_eq!(receipt.source().sequence, Some(37));
        assert!(receipt.source().response_id.is_none());
        assert!(receipt.source().completion_source_id.is_none());
        assert_eq!(receipt.source().namespace, SourceNamespace::Public);
    }
}

#[test]
fn string_and_explicit_null_cursor_follow_the_original_input_producer() {
    let item =
        json!({"type":"message","role":"user","content":[{"type":"input_text","text":"hello 雪"}]});
    let expected = proof(std::slice::from_ref(&item));
    for body in [
        json!({"input":"hello 雪"}),
        json!({"previous_response_id":null,"input":"hello 雪"}),
    ] {
        let mut graph = input_graph(&[expected.clone()]);
        graph
            .push_public_request_input(test_flow(), Uuid::now_v7(), 1, &frame(&body))
            .unwrap();
        let result = graph.resolve().unwrap();
        assert!(result.unresolved_targets.is_empty());
        assert_eq!(
            result.verified[0].replacement().current(),
            current(std::slice::from_ref(&item))
        );
    }
}

#[test]
fn request_only_never_matches_an_intermediate_prefix_or_missing_input() {
    let first = json!({"role":"user","content":"first"});
    let second = json!({"role":"user","content":"second"});
    let first_proof = proof(std::slice::from_ref(&first));
    let two_proof = proof(&[first.clone(), second.clone()]);
    for (expected, body) in [
        (first_proof.clone(), json!({"input":[first,second]})),
        (two_proof.clone(), json!({"input":[first]})),
        (two_proof.clone(), json!({"input":[second,first]})),
        (
            first_proof.clone(),
            json!({"messages":[first],"preview":"first"}),
        ),
        (first_proof.clone(), json!({"input":null})),
        (
            first_proof.clone(),
            json!({"input":{"role":"user","content":"first"}}),
        ),
        (
            first_proof.clone(),
            json!({"previous_response_id":"owned-or-not-it-is-a-cursor","input":[first]}),
        ),
        (
            first_proof.clone(),
            json!({"previous_response_id":42,"input":[first]}),
        ),
        (
            first_proof.clone(),
            json!({"input":[{"role":"user","content":"changed"}]}),
        ),
    ] {
        let mut graph = input_graph(&[expected.clone()]);
        graph
            .push_public_request_input(test_flow(), Uuid::now_v7(), 1, &frame(&body))
            .unwrap();
        let result = graph.resolve().unwrap();
        assert!(result.verified.is_empty());
        assert_eq!(result.unresolved_targets, vec![target(expected)]);
        assert_eq!(result.unresolved_sources.len(), 1);
    }
}

#[test]
fn malformed_wrong_flow_or_wrong_sequence_original_frame_cannot_attest() {
    let item = json!({"role":"user","content":"hello"});
    let expected = proof(std::slice::from_ref(&item));
    for original in [
        json!({"frame_kind":"request","encoding":"utf8","body":"{bad JSON"}),
        json!({"frame_kind":"request","encoding":"utf8","body":"[]"}),
        json!({"frame_kind":"response_json","encoding":"utf8","body":json!({"input":[item]}).to_string()}),
        json!({"frame_kind":"request","encoding":"base64","body":"invalid%%%"}),
        json!({"frame_kind":"request","encoding":"utf8","body":json!({"input":[item]}).to_string(),"sequence":99}),
    ] {
        let mut graph = input_graph(&[expected.clone()]);
        graph
            .push_public_request_input(test_flow(), Uuid::now_v7(), 1, &original)
            .unwrap();
        let result = graph.resolve().unwrap();
        assert!(result.verified.is_empty());
        assert_eq!(result.unresolved_targets.len(), 1);
    }
    let mut graph = input_graph(&[expected.clone()]);
    graph
        .push_public_request_input(
            Uuid::from_u128(2),
            Uuid::now_v7(),
            1,
            &frame(&json!({"input":[item]})),
        )
        .unwrap();
    let result = graph.resolve().unwrap();
    assert!(result.verified.is_empty());
    assert_eq!(result.unresolved_targets, vec![target(expected)]);
}

#[test]
fn same_hash_full_obligation_remains_until_a_completed_source_upgrades_coverage() {
    let input = json!({"role":"user","content":"hello"});
    let expected = proof(std::slice::from_ref(&input));
    let scoped = target(expected.clone());
    let mut graph =
        HistorySourceGraph::with_request_input_targets(&[scoped.clone()], &[scoped.clone()])
            .unwrap();
    graph
        .push_public_request_input(
            test_flow(),
            Uuid::now_v7(),
            1,
            &frame(&json!({"input":[input]})),
        )
        .unwrap();
    let result = graph.resolve().unwrap();
    assert_eq!(result.verified[0].coverage(), SourceCoverage::RequestInput);
    assert_eq!(
        result.unresolved_targets,
        vec![scoped.clone()],
        "non-input occurrence still needs full source"
    );
    for input_first in [true, false] {
        let mut graph =
            HistorySourceGraph::with_request_input_targets(&[scoped.clone()], &[scoped.clone()])
                .unwrap();
        if input_first {
            graph
                .push_public_request_input(
                    test_flow(),
                    Uuid::now_v7(),
                    1,
                    &frame(&json!({"input":[input]})),
                )
                .unwrap();
        }
        feed_round(
            &mut graph,
            test_flow(),
            Uuid::now_v7(),
            json!({"input":[input]}),
            &[],
            "completed",
        );
        if !input_first {
            graph
                .push_public_request_input(
                    test_flow(),
                    Uuid::now_v7(),
                    1,
                    &frame(&json!({"input":[input]})),
                )
                .unwrap();
        }
        let result = graph.resolve().unwrap();
        assert!(result.unresolved_targets.is_empty());
        assert_eq!(result.verified[0].coverage(), SourceCoverage::FullHistory);
        assert!(result.verified[0].source().completion_source_id.is_some());
    }
    // Existing complete sources can attest input slots without a full-only target.
    let mut graph = input_graph(&[expected]);
    feed_round(
        &mut graph,
        test_flow(),
        Uuid::now_v7(),
        json!({"input":[input]}),
        &[],
        "completed-only",
    );
    let result = graph.resolve().unwrap();
    assert!(result.unresolved_targets.is_empty());
    assert_eq!(result.verified[0].coverage(), SourceCoverage::FullHistory);
}

#[test]
fn failed_native_completion_does_not_promote_input_receipt_to_output_evidence() {
    let input = json!({"role":"user","content":"hello"});
    let expected = proof(std::slice::from_ref(&input));
    let scoped = target(expected);
    let mut graph =
        HistorySourceGraph::with_request_input_targets(&[scoped.clone()], &[scoped.clone()])
            .unwrap();
    let node = Uuid::now_v7();
    let invocation = Uuid::now_v7();
    graph
        .push_native(
            test_flow(),
            Some(node),
            Uuid::now_v7(),
            1,
            "provider_semantic_step",
            &prepared(json!({"input":[input]}), invocation, 0),
        )
        .unwrap();
    let mut failed = received("failed-response", invocation, 0);
    failed["status"] = json!("incomplete");
    graph
        .push_native(
            test_flow(),
            Some(node),
            Uuid::now_v7(),
            2,
            "provider_semantic_step",
            &failed,
        )
        .unwrap();
    graph.finish_native_flow(test_flow()).unwrap();
    graph
        .push_public_request_input(
            test_flow(),
            Uuid::now_v7(),
            1,
            &frame(&json!({"input":[input]})),
        )
        .unwrap();
    let result = graph.resolve().unwrap();
    assert_eq!(result.verified[0].coverage(), SourceCoverage::RequestInput);
    assert_eq!(result.unresolved_targets, vec![scoped]);
    assert!(result
        .unresolved_sources
        .iter()
        .any(|source| source.reason.contains("completion_unsuccessful")));
}

#[test]
fn request_input_v1_ambiguity_still_poisoned_including_across_coverage_kinds() {
    let alpha = json!({"type":"future","id":"alpha"});
    let beta = json!({"type":"future","id":"beta"});
    let (old, _) = legacy_v1(&alpha);
    let mut graph = input_graph(&[old.clone()]);
    graph
        .push_public_request_input(
            test_flow(),
            Uuid::now_v7(),
            1,
            &frame(&json!({"input":[alpha]})),
        )
        .unwrap();
    assert!(graph
        .push_public_request_input(
            test_flow(),
            Uuid::now_v7(),
            2,
            &frame(&json!({"input":[beta]}))
        )
        .is_err());
    assert!(graph.resolve().is_err());
    let scoped = target(old);
    let mut graph =
        HistorySourceGraph::with_request_input_targets(&[scoped.clone()], &[scoped]).unwrap();
    feed_round(
        &mut graph,
        test_flow(),
        Uuid::now_v7(),
        json!({"input":[alpha]}),
        &[],
        "completed",
    );
    assert!(graph
        .push_public_request_input(
            test_flow(),
            Uuid::now_v7(),
            2,
            &frame(&json!({"input":[beta]}))
        )
        .is_err());
    assert!(graph.resolve().is_err());
}

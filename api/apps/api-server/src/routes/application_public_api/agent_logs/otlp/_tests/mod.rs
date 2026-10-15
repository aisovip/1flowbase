use super::*;
use extension_contracts::ManagedProjectionContract;
use serde_json::json;

#[test]
fn otlp_managed_contracts_expose_only_counts_and_static_wire_tags() {
    let input = Input {
        format: WireFormat::Json,
        encoding: ContentEncoding::Gzip,
        body: Bytes::from_static(b"private body with raw source facts and bearer-like text"),
    };
    let projection = input.project_for_managed_hook().unwrap();
    assert_eq!(
        projection,
        json!({"kind":Input::CONTRACT_ID,"byte_count":input.body.len(),"format":"json","encoding":"gzip"})
    );
    let output = Output {
        response: ExportTraceServiceResponse {
            partial_success: Some(ExportTracePartialSuccess {
                rejected_spans: 3,
                error_message: "not projected".into(),
            }),
        },
        accepted_spans: 2,
    };
    let error = TargetError(ControlPlaneError::NotAuthenticated.into());
    for (id, schema, value) in [
        (
            Input::CONTRACT_ID,
            Input::managed_projection_schema().unwrap(),
            projection,
        ),
        (
            Output::CONTRACT_ID,
            Output::managed_projection_schema().unwrap(),
            output.project_for_managed_hook().unwrap(),
        ),
        (
            TargetError::CONTRACT_ID,
            TargetError::managed_projection_schema().unwrap(),
            error.project_for_managed_hook().unwrap(),
        ),
    ] {
        let contract = ManagedProjectionContract {
            contract_id: id.into(),
            contract_version: "1".into(),
            schema,
        }
        .compile_for_registration()
        .unwrap();
        contract.validate(&value).unwrap();
        let mut leaked = value.clone();
        leaked["body"] = json!("private source");
        assert!(contract.validate(&leaked).is_err());
        assert!(
            !value.to_string().contains("private") && !value.to_string().contains("not projected")
        );
    }
}

#[test]
fn partial_attribution_counts_input_spans_not_events_or_deduplicated_spans() {
    let mut request = agent_logs_protocol::decode(
        "application/json",
        None,
        include_bytes!("../../../../../agent_logs_protocol/_tests/fixtures/openlit.json"),
    )
    .unwrap();
    let span = request.resource_spans[0].scope_spans[0].spans[1].clone();
    request.resource_spans[0].scope_spans[0].spans.push(span);
    let (batches, counts) = validated_batches(&request).unwrap();
    assert_eq!(counts.values().sum::<usize>(), 5);
    assert!(
        batches
            .iter()
            .map(|batch| batch.events.len())
            .sum::<usize>()
            > 5
    );
    assert_eq!(counts.values().copied().collect::<Vec<_>>(), [4, 1]);
}

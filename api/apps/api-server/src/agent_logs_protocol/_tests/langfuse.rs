use super::*;

#[test]
fn langfuse_api_self_metadata_replays_exact_source_facts_and_deduplicates() {
    let expected = source_batches();
    let request = batches_to_traces(&expected).unwrap();
    let mut observations = Vec::new();
    for span in &request.resource_spans[0].scope_spans[0].spans {
        let metadata = span
            .attributes
            .iter()
            .find(|attr| attr.key == "langfuse.observation.metadata")
            .unwrap();
        let Some(any_value::Value::StringValue(metadata)) = &metadata.value.as_ref().unwrap().value
        else {
            panic!("metadata blob");
        };
        observations.push(json!({"metadata":serde_json::from_str::<Value>(metadata).unwrap()}));
    }
    observations.push(observations[0].clone());
    let response = json!({"data":observations,"meta":{}});
    assert_eq!(
        serde_json::to_value(lf_observations_to_batches(&response).unwrap()).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert!(lf_observations_to_batches(
        &json!({"metadata":{"1flowbase_agent_log":null},"id":"fallback-prohibited"})
    )
    .is_err());
    assert!(lf_observations_to_batches(
        &json!({"metadata":"{\"1flowbase_agent_log\":invalid","id":"fallback-prohibited"})
    )
    .is_err());
}

#[test]
fn generic_langfuse_observation_preserves_raw_inputs_usage_and_real_execution_identity() {
    let observation = json!({"id":"real-lf-observation","traceId":"real-lf-trace","sessionId":"real-lf-session","parentObservationId":"real-lf-parent","startTime":"2026-10-07T08:00:00.123456789Z","endTime":"2026-10-07T08:00:01.987654321Z","name":"observed generation","type":"GENERATION","model":"actual-model","provider":"actual-provider","input":[{"role":"user","content":"history"}],"output":"plain assistant output","usageDetails":{"input":5,"output":7,"total":12,"cache_read_input_tokens":2,"cache_creation_input_tokens":3},"metadata":{"unknown_original":{"keep":true}}});
    let batches = lf_observations_to_batches(&observation).unwrap();
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].events.len(), 3);
    assert!(batches[0]
        .events
        .iter()
        .all(|event| event.source_session_id == "real-lf-session"
            && event.source_task_id == "real-lf-trace/real-lf-observation"
            && event.parent_source_task_id.as_deref() == Some("real-lf-trace/real-lf-parent")
            && event.phase.is_none()
            && event.kind != AgentLogEventKind::TaskEnd
            && event.raw["langfuse_observation"] == observation));
    assert!(batches[0]
        .events
        .iter()
        .any(|event| event.kind == AgentLogEventKind::Context
            && event.content.as_deref() == Some("history")));
    assert!(batches[0]
        .events
        .iter()
        .any(|event| event.kind == AgentLogEventKind::Assistant
            && event.content.as_deref() == Some("plain assistant output")));
    let usage = batches[0]
        .events
        .iter()
        .find_map(|event| event.usage.as_ref())
        .unwrap();
    assert_eq!(usage.basis, AgentLogUsageBasis::Cumulative);
    assert_eq!(
        usage.response_id.as_deref(),
        Some("real-lf-trace/real-lf-observation")
    );
    assert_eq!(usage.total_tokens, Some(12));
    assert_eq!(usage.input_cache_hit_tokens, Some(2));
    let repeated =
        lf_observations_to_batches(&json!([observation.clone(), observation.clone()])).unwrap();
    assert_eq!(
        serde_json::to_value(repeated).unwrap(),
        serde_json::to_value(&batches).unwrap()
    );
    let mut missing_id = observation.clone();
    missing_id.as_object_mut().unwrap().remove("id");
    assert!(lf_observations_to_batches(&missing_id).is_err());
    let mut missing_time = observation;
    missing_time.as_object_mut().unwrap().remove("startTime");
    assert!(lf_observations_to_batches(&missing_time).is_err());
}

#[test]
fn generic_langfuse_otlp_blob_usage_is_cumulative_and_tool_api_is_observed_execution() {
    let mut request = traces_fixture();
    let span = first_span(&mut request);
    span.attributes
        .retain(|attr| !attr.key.starts_with("gen_ai.usage."));
    string_attr(
        span,
        "langfuse.observation.usage_details",
        r#"{"input":6,"output":4,"total":10}"#,
    );
    let batches = traces_to_batches(&request).unwrap();
    let usage = batches
        .iter()
        .flat_map(|batch| &batch.events)
        .find_map(|event| event.usage.as_ref())
        .unwrap();
    assert_eq!(usage.total_tokens, Some(10));
    assert_eq!(usage.basis, AgentLogUsageBasis::Cumulative);
    let observation = json!({"id":"tool-id","traceId":"trace-id","startTime":"2026-10-07T08:00:00Z","type":"TOOL","name":"weather","input":{"city":"杭州"},"output":{"temp":20}});
    let batches = lf_observations_to_batches(&observation).unwrap();
    assert!(batches[0]
        .events
        .iter()
        .any(|event| event.kind == AgentLogEventKind::ToolCall
            && event.name.as_deref() == Some("weather")));
    assert!(batches[0]
        .events
        .iter()
        .any(|event| event.kind == AgentLogEventKind::ToolResult
            && event.content.as_deref() == Some("{\"temp\":20}")));
}

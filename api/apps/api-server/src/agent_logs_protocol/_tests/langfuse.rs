use super::*;
use base64::{engine::general_purpose::STANDARD, Engine as _};

// Langfuse packages/shared/src/server/otel/utils.ts: collectJsonPaths recurses
// into objects, preserves string/null leaves, JSON-encodes other leaves, and
// keeps the first value when literal dotted keys collide with nested paths.
fn langfuse_persisted_metadata(metadata: &Value) -> Value {
    fn collect(value: &Value, prefix: &str, output: &mut serde_json::Map<String, Value>) {
        for (key, value) in value.as_object().unwrap() {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            if value.is_object() {
                collect(value, &path, output);
            } else if !output.contains_key(&path) {
                let stored = match value {
                    Value::Null | Value::String(_) => value.clone(),
                    _ => Value::String(value.to_string()),
                };
                output.insert(path, stored);
            }
        }
    }
    let mut flattened = serde_json::Map::new();
    collect(metadata, "", &mut flattened);
    Value::Object(flattened)
}

fn exported_metadata(span: &Span) -> Value {
    let attribute = span
        .attributes
        .iter()
        .find(|attribute| attribute.key == "langfuse.observation.metadata")
        .unwrap();
    let Some(any_value::Value::StringValue(text)) = &attribute.value.as_ref().unwrap().value else {
        panic!("metadata JSON blob");
    };
    serde_json::from_str(text).unwrap()
}

#[test]
fn langfuse_persisted_scalar_preserves_colliding_raw_paths_and_three_original_records() {
    let mut expected = source_batches();
    for event in &mut expected[0].events {
        event.raw = json!({"db":{"host":"nested value"},"db.host":"literal dotted key","array":[null,true,123,{"a.b":"literal","a":{"b":"nested"}}],"empty":{},"null":null,"nul":"source\u{0}text","long":"完整来源".repeat(512),"original":event.raw});
    }
    let request = batches_to_traces(&expected).unwrap();
    let spans = &request.resource_spans[0].scope_spans[0].spans;
    let mut observations = Vec::new();
    for (span, event) in spans.iter().zip(&expected[0].events) {
        let metadata = exported_metadata(span);
        let persisted = langfuse_persisted_metadata(&metadata);
        assert_eq!(
            persisted, metadata,
            "the replay scalar must survive Langfuse path storage unchanged"
        );
        assert!(persisted["1flowbase_agent_log"]
            .as_str()
            .unwrap()
            .starts_with("1flowbase.agent-logs.source-fact/v1;base64,"));
        // Show why decoding the former object by heuristic unflattening cannot
        // preserve both values: the actual storage algorithm has discarded one.
        let legacy = json!({"1flowbase_agent_log":{"schema_version":expected[0].schema_version,"source_id":expected[0].source_id,"source_client":expected[0].source_client,"event":event}});
        let flattened_legacy = langfuse_persisted_metadata(&legacy);
        assert_eq!(
            flattened_legacy["1flowbase_agent_log.event.raw.db.host"],
            "nested value"
        );
        assert!(!flattened_legacy.to_string().contains("literal dotted key"));
        observations.push(json!({"id":hex_id(&span.span_id),"traceId":hex_id(&span.trace_id),"startTime":event.occurred_at,"metadata":persisted}));
    }
    observations.push(observations[0].clone());
    let restored = lf_observations_to_batches(&json!({"data":observations,"meta":{}})).unwrap();
    assert_eq!(
        serde_json::to_value(&restored).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
    assert_eq!(
        restored
            .iter()
            .map(|batch| batch.events.len())
            .sum::<usize>(),
        4
    );
    let identities = restored
        .iter()
        .flat_map(|batch| {
            batch.events.iter().map(move |event| {
                (
                    &batch.source_id,
                    &batch.source_client,
                    &event.source_session_id,
                    &event.source_task_id,
                )
            })
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        identities.len(),
        3,
        "Langfuse storage must not invent a record for each observation"
    );
}

fn hex_id(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn corrupted_encoded_or_flattened_self_metadata_never_becomes_generic_source() {
    let expected = source_batches();
    let request = batches_to_traces(&expected).unwrap();
    let metadata = exported_metadata(&request.resource_spans[0].scope_spans[0].spans[0]);
    let encoded = metadata["1flowbase_agent_log"].as_str().unwrap();
    let prefix = "1flowbase.agent-logs.source-fact/v1;base64,";
    let unknown_schema = json!({"schema_version":"unknown","source_id":"collector-a","source_client":"codex","event":expected[0].events[0]});
    let legacy = json!({"1flowbase_agent_log":{"schema_version":expected[0].schema_version,"source_id":expected[0].source_id,"source_client":expected[0].source_client,"event":expected[0].events[0]}});
    for metadata in [
        json!({"1flowbase_agent_log":format!("{prefix}%%%")}),
        json!({"1flowbase_agent_log":&encoded[..encoded.len()-5]}),
        json!({"1flowbase_agent_log":encoded.replace("/v1;","/v2;")}),
        json!({"1flowbase_agent_log":encoded.replace(";base64,",";hex,")}),
        json!({"1flowbase_agent_log":format!("{prefix}{}",STANDARD.encode(b"{\"truncated\":"))}),
        json!({"1flowbase_agent_log":format!("{prefix}{}",STANDARD.encode(unknown_schema.to_string()))}),
        langfuse_persisted_metadata(&legacy),
        json!({"1flowbase_agent_log.event.raw.db.host":"lost source"}),
    ] {
        // This complete generic observation would succeed if the self marker
        // were silently ignored, making rejection a discriminating assertion.
        let observation = json!({"id":"valid-observation","traceId":"valid-trace","startTime":"2026-10-07T08:00:00Z","type":"SPAN","metadata":metadata});
        assert!(lf_observations_to_batches(&observation).is_err());
        let mut otlp = request.clone();
        string_attr(
            &mut otlp.resource_spans[0].scope_spans[0].spans[0],
            "langfuse.observation.metadata",
            &metadata.to_string(),
        );
        assert!(traces_to_batches(&otlp).is_err());
    }
    let generic = json!({"id":"valid-observation","traceId":"valid-trace","startTime":"2026-10-07T08:00:00Z","type":"SPAN","metadata":{"ordinary.field":"unrelated"}});
    assert!(lf_observations_to_batches(&generic).is_ok());
}

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

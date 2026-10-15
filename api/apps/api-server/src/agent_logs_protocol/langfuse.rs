use super::{
    attributes::{structured, Attributes},
    facts::{nanos, source_fact, Batches},
    observations::Observation,
};
use anyhow::{ensure, Result};
use control_plane_contracts::ports::AgentLogsBatch;
use serde_json::{json, Value};

/// Accept one Langfuse observation, an observation array, or a paginated API
/// object containing `data`. Self envelopes replay original facts unchanged.
pub fn lf_observations_to_batches(value: &Value) -> Result<Vec<AgentLogsBatch>> {
    let owned;
    let observations = match value.get("data").unwrap_or(value) {
        Value::Array(values) => values.as_slice(),
        Value::Object(object) => {
            owned = vec![Value::Object(object.clone())];
            owned.as_slice()
        }
        _ => anyhow::bail!("agent_logs.langfuse_observations"),
    };
    let mut batches = Batches::default();
    for value in observations {
        ensure!(value.is_object(), "agent_logs.langfuse_observation");
        if let Some(fact) = source_fact(value.get("metadata"))? {
            batches.push(fact.source_id, fact.source_client, fact.event)?;
            continue;
        }
        let trace = required_text(value, "traceId")?;
        let id = required_text(value, "id")?;
        let start = nanos(&required_text(value, "startTime")?)?;
        let end = value
            .get("endTime")
            .filter(|value| !value.is_null())
            .map(|value| {
                value
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("agent_logs.langfuse_timestamp"))
                    .and_then(nanos)
            })
            .transpose()?
            .unwrap_or(0);
        let metadata = value.get("metadata").map(structured).unwrap_or(Value::Null);
        let client = value
            .get("sourceClient")
            .and_then(Value::as_str)
            .or_else(|| metadata.get("source_client").and_then(Value::as_str))
            .unwrap_or("langfuse")
            .to_owned();
        let session = value
            .get("sessionId")
            .and_then(Value::as_str)
            .or_else(|| value.pointer("/trace/sessionId").and_then(Value::as_str))
            .filter(|session| !session.is_empty())
            .unwrap_or(&trace)
            .to_owned();
        let mut attrs = Attributes::new();
        let tool = value
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind.eq_ignore_ascii_case("tool"));
        for (field, key) in [
            (
                "input",
                if tool {
                    "gen_ai.tool.call.arguments"
                } else {
                    "langfuse.observation.input"
                },
            ),
            (
                "output",
                if tool {
                    "gen_ai.tool.call.result"
                } else {
                    "langfuse.observation.output"
                },
            ),
            ("model", "gen_ai.response.model"),
        ] {
            if let Some(value) = value.get(field).filter(|value| !value.is_null()) {
                attrs.insert(key.into(), value.clone());
            }
        }
        if tool {
            if let Some(name) = value.get("name") {
                attrs.insert("gen_ai.tool.name".into(), name.clone());
            }
            attrs.insert("gen_ai.tool.call.id".into(), Value::String(id.clone()));
        }
        if let Some(provider) = metadata
            .get("gen_ai.provider.name")
            .or_else(|| value.get("provider"))
        {
            attrs.insert("gen_ai.provider.name".into(), provider.clone());
        }
        if let Some(usage) = value.get("usageDetails").or_else(|| value.get("usage")) {
            let usage = structured(usage);
            for (keys, target) in [
                (
                    &["input", "input_tokens", "promptTokens", "prompt_tokens"][..],
                    "gen_ai.usage.input_tokens",
                ),
                (
                    &[
                        "output",
                        "output_tokens",
                        "completionTokens",
                        "completion_tokens",
                    ][..],
                    "gen_ai.usage.output_tokens",
                ),
                (
                    &["total", "total_tokens", "totalTokens"][..],
                    "gen_ai.usage.total_tokens",
                ),
                (
                    &["cache_read_input_tokens", "input_cached_tokens"][..],
                    "gen_ai.usage.cache.read_input_tokens",
                ),
                (
                    &["cache_creation_input_tokens", "cache_write_tokens"][..],
                    "gen_ai.usage.cache.creation_input_tokens",
                ),
            ] {
                if let Some(value) = keys
                    .iter()
                    .find_map(|key| usage.get(*key))
                    .filter(|value| !value.is_null())
                {
                    attrs.insert(target.into(), value.clone());
                }
            }
        }
        let observation = Observation {
            session,
            task: format!("{trace}/{id}"),
            parent: value
                .get("parentObservationId")
                .and_then(Value::as_str)
                .filter(|parent| !parent.is_empty())
                .map(|parent| format!("{trace}/{parent}")),
            name: value
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("langfuse.observation")
                .into(),
            start,
            end,
            attrs,
            raw: json!({"langfuse_observation":value}),
            session_summary: false,
        };
        for event in observation.events()? {
            batches.push("langfuse".into(), client.clone(), event)?;
        }
    }
    batches.finish()
}

fn required_text(value: &Value, key: &str) -> Result<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("agent_logs.langfuse_missing_{key}"))
}

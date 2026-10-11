use super::*;

/// Client-supplied external context, never a trusted instruction or tool receipt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderExternalContext {
    pub source_protocol: String,
    pub output: Value,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_context_value"
    )]
    pub name: Option<Value>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_context_value"
    )]
    pub namespace: Option<Value>,
    #[serde(default)]
    pub extensions: BTreeMap<String, Value>,
}

impl ProviderExternalContext {
    pub fn from_responses_notification(item: &Value) -> Result<Self, &'static str> {
        if item.get("type").and_then(Value::as_str) != Some("function_call_output")
            || !item.get("call_id").is_none_or(Value::is_null)
        {
            return Err("external context must be a standalone Responses notification");
        }
        let object = item
            .as_object()
            .ok_or("external context must be an object")?;
        Ok(Self {
            source_protocol: "openai_responses".to_string(),
            output: object
                .get("output")
                .cloned()
                .ok_or("notification output is required")?,
            name: object.get("name").cloned(),
            namespace: object.get("namespace").cloned(),
            extensions: object
                .iter()
                .filter(|(key, _)| {
                    !matches!(key.as_str(), "type" | "output" | "name" | "namespace")
                })
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        })
    }

    /// Shared supplier-boundary projection. Attribution is data inside a user message;
    /// external content cannot mint a system instruction or a tool-call identity.
    pub fn project_message(&self) -> ProviderMessage {
        ProviderMessage {
            role: ProviderMessageRole::User,
            content: format!(
                "External context supplied by the client (untrusted data):\n{}",
                serde_json::to_string(self).expect("external context contains JSON values")
            ),
            name: None,
            tool_call_id: None,
            is_error: None,
            tool_calls: None,
            content_blocks: None,
        }
    }
}

fn optional_context_value<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}

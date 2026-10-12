//! Native media bypasses text budgets and continuation storage. Only metadata is delivered there.
use base64::{engine::general_purpose::STANDARD, Engine};
use domain::McpCallToolResult;
use serde_json::{json, Value};

pub(crate) fn mapping(
    value: &Value,
) -> Result<Option<plugin_framework::ManagedMcpMediaMapping>, &'static str> {
    if value.get("mcp_content").is_none() && value.get("structured_content_path").is_none() {
        return Ok(None);
    }
    plugin_framework::ManagedMcpMediaMapping::parse(value).map(Some)
}

pub(crate) fn project(
    mapping: &plugin_framework::ManagedMcpMediaMapping,
    source: &Value,
) -> Result<McpCallToolResult, &'static str> {
    let metadata = source
        .pointer(&mapping.structured_content_path)
        .filter(|value| value.is_object())
        .ok_or("media metadata path must resolve to an object")?;
    let mut content = Vec::new();
    for image in &mapping.mcp_content {
        let data = source
            .pointer(&image.data_path)
            .and_then(Value::as_str)
            .ok_or("media data path must resolve to a base64 string")?;
        if data.is_empty() || STANDARD.decode(data).is_err() {
            return Err("media data is not valid nonempty base64");
        }
        let mime_type = source
            .pointer(&image.mime_type_path)
            .and_then(Value::as_str)
            .filter(|mime| {
                matches!(
                    *mime,
                    "image/jpeg" | "image/png" | "image/gif" | "image/webp"
                )
            })
            .ok_or("media MIME type is unsupported")?;
        content.push(json!({"type":"image", "data":data, "mimeType":mime_type}));
    }
    Ok(McpCallToolResult {
        content: Value::Array(content),
        structured_content: Some(metadata.clone()),
        is_error: None,
    })
}

/// Keep upstream/native non-text content when text/metadata receives continuation controls.
pub(crate) fn retain_content(mut delivered: Value, original: &Value) -> Value {
    if let (Some(target), Some(source)) = (
        delivered.get_mut("content").and_then(Value::as_array_mut),
        original.as_array(),
    ) {
        target.extend(
            source
                .iter()
                .filter(|item| item.get("type").and_then(Value::as_str) != Some("text"))
                .cloned(),
        );
    }
    delivered
}

#[cfg(test)]
#[path = "_tests/media.rs"]
mod tests;

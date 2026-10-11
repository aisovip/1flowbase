//! Responses history equivalence v3.
//!
//! Preserve the v2 contract except for the optional function-call delivery ID.
//! AI SDK replays the call without this output item ID; correlation remains
//! bound to call_id. Arguments, opaque fields and other item IDs stay evidence.
//! Source: pinned @ai-sdk/openai 3.0.84 responses/openai-responses-api.ts
//! OpenAIResponsesFunctionCall declares id?: string; convert-to-openai-responses-input.ts
//! emits ordinary function calls without id. The captured native replay confirms this.
use anyhow::Result;
use serde_json::Value;

pub(super) fn normalize_item(item: &Value) -> Result<Value> {
    let mut item = super::legacy_v2::normalize_item(item)?;
    let object = item
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("native_history_item_invalid"))?;
    if object.get("type").and_then(Value::as_str) == Some("function_call")
        && object
            .get("id")
            .is_some_and(|id| id.is_null() || id.is_string())
    {
        // This is an optional delivery identity, never the required call_id.
        // Malformed non-string IDs remain evidence rather than being erased.
        object.remove("id");
    }
    Ok(item)
}

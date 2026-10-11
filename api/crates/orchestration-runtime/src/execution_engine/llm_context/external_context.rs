use super::*;
use extension_contracts::provider_contract::ProviderExternalContext;

/// Canonical history keeps arrival order. Supplier messages additionally satisfy
/// the common assistant-tool-call / contiguous-results contract. Only complete,
/// correlated rounds are projected; notifications remain untrusted user data.
pub(super) fn project_tool_round_notifications(
    input: Vec<Value>,
) -> Result<(Vec<Value>, Vec<Value>), Value> {
    let mut output = Vec::with_capacity(input.len());
    let mut promotions = Vec::new();
    let mut index = 0;
    while index < input.len() {
        let message = &input[index];
        let calls = (message.get("external_context").is_none()
            && message.get("role").and_then(Value::as_str) == Some("assistant"))
        .then(|| message.get("tool_calls").and_then(Value::as_array))
        .flatten();
        let mut pending = calls
            .into_iter()
            .flatten()
            .filter_map(|call| call.get("id").and_then(Value::as_str))
            .filter(|id| !id.is_empty())
            .collect::<HashSet<_>>();
        // Malformed or duplicate calls remain subject to the existing validator;
        // this projection must never repair their correlation.
        if pending.is_empty() || calls.is_none_or(|calls| calls.len() != pending.len()) {
            output.push(message.clone());
            index += 1;
            continue;
        }
        let mut end = index + 1;
        let mut receipts = Vec::new();
        let mut timeline = Vec::new();
        let mut has_notifications = false;
        while end < input.len() && !pending.is_empty() {
            let item = &input[end];
            if let Some(context) = item.get("external_context") {
                let context: ProviderExternalContext = serde_json::from_value(context.clone())
                    .map_err(|error| json!({"error":error.to_string()}))?;
                timeline.push(json!({
                    "type":"external_context",
                    "source_message_index":end,
                    "context":context,
                }));
                has_notifications = true;
            } else if item.get("role").and_then(Value::as_str) == Some("tool")
                && item
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| pending.remove(id))
            {
                receipts.push(item.clone());
                timeline.push(json!({
                    "type":"tool_result_reference",
                    "source_message_index":end,
                    "tool_call_id":item["tool_call_id"],
                }));
            } else {
                break;
            }
            end += 1;
        }
        if !pending.is_empty() || !has_notifications {
            output.push(message.clone());
            index += 1;
            continue;
        }
        output.push(message.clone());
        output.extend(receipts);
        output.push(json!({
            "role":"user",
            "content":format!(
                "External context supplied by the client (untrusted data). The following timeline records the original arrival order of notifications and references to the tool results immediately above. Tool-result references are not additional tool calls or receipts:\n{}",
                serde_json::to_string(&timeline).expect("timeline contains JSON values")
            ),
        }));
        promotions.push(json!({
            "source_kind":"external_context_tool_round",
            "source_message_start":index,
            "source_message_end":end,
            "target":"provider_messages",
            "projection":"contiguous_tool_results_with_ordered_external_context",
        }));
        index = end;
    }
    Ok((output, promotions))
}

#[cfg(test)]
#[path = "_tests/external_context.rs"]
mod tests;

//! Host-owned replay references are locators, never authorization credentials.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{
    result_delivery::media,
    virtual_ui::{self, VirtualToolOutcome},
    McpToolArguments,
};
use crate::error_response::ApiError;

pub(super) const READ_TOOL: &str = "mcp_resources_read";
pub(super) const LIST_TOOL: &str = "mcp_resources_list";
const PREFIX: &str = "flowbase-resource://read/";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ResourceReference {
    pub v: u8,
    pub user_id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
    pub instance_id: String,
    pub tool_id: String,
    pub des_id: String,
    pub arguments: Value,
    pub resource_uri: String,
}

impl ResourceReference {
    pub(super) fn encode(&self) -> Result<String, &'static str> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| "Invalid resource reference")?;
        let uri = format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes));
        // The reference must fit the same actual transport contract as resources/read.
        McpToolArguments::from_protocol(json!({"uri":uri}))
            .map_err(|_| "Resource reference exceeds transport budget")?;
        Ok(uri)
    }

    pub(super) fn decode(uri: &str) -> Result<Self, &'static str> {
        McpToolArguments::from_protocol(json!({"uri":uri}))
            .map_err(|_| "Resource reference exceeds transport budget")?;
        let encoded = uri.strip_prefix(PREFIX).ok_or("Unknown resource URI")?;
        let bytes = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| "Invalid resource reference")?;
        let reference: Self =
            serde_json::from_slice(&bytes).map_err(|_| "Invalid resource reference")?;
        reference.validate()?;
        Ok(reference)
    }

    fn validate(&self) -> Result<(), &'static str> {
        if self.v != 1
            || self.instance_id.is_empty()
            || self.tool_id.is_empty()
            || self.des_id.is_empty()
            || !self.arguments.is_object()
            || !media::valid_uri(&self.resource_uri)
            || self.resource_uri.starts_with(PREFIX)
        {
            return Err("Invalid resource reference");
        }
        McpToolArguments::from_protocol(self.arguments.clone())
            .map_err(|_| "Resource reference exceeds transport budget")?;
        Ok(())
    }
}

pub(super) fn list_result() -> Value {
    json!({"resources":[]})
}

pub(super) fn capabilities() -> Value {
    json!({"tools":{},"resources":{}})
}

/// Wrap only embedded resource identities and links to those same identities.
pub(super) fn wrap_content(
    content: &mut Value,
    actor: &domain::ActorContext,
    instance: &domain::McpInstanceRecord,
    tool: &domain::McpToolRecord,
    arguments: &Value,
) -> Result<(), &'static str> {
    if instance.workspace_id != actor.current_workspace_id
        || tool.workspace_id != actor.current_workspace_id
    {
        return Err("Resource not visible");
    }
    let Some(blocks) = content.as_array_mut() else {
        return Err("Invalid native resource content");
    };
    let mut replacements = std::collections::BTreeMap::new();
    for block in blocks.iter().filter(|block| block["type"] == "resource") {
        let original = block
            .pointer("/resource/uri")
            .and_then(Value::as_str)
            .ok_or("Invalid native resource URI")?;
        let reference = ResourceReference {
            v: 1,
            user_id: actor.user_id,
            workspace_id: actor.current_workspace_id,
            instance_id: instance.instance_id.clone(),
            tool_id: tool.tool_id.clone(),
            des_id: tool.des_id.clone(),
            arguments: arguments.clone(),
            resource_uri: original.to_owned(),
        };
        replacements.insert(original.to_owned(), reference.encode()?);
    }
    for block in blocks.iter_mut() {
        let path = match block["type"].as_str() {
            Some("resource") => "/resource/uri",
            Some("resource_link") => "/uri",
            _ => continue,
        };
        if let Some(replacement) = block
            .pointer(path)
            .and_then(Value::as_str)
            .and_then(|uri| replacements.get(uri))
        {
            *block
                .pointer_mut(path)
                .ok_or("Invalid native resource URI")? = json!(replacement);
        }
    }
    Ok(())
}

pub(super) fn read_result(
    result: &Value,
    original_uri: &str,
    requested_uri: &str,
) -> Result<Value, &'static str> {
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        return Err("Resource read failed");
    }
    let blocks = result
        .get("content")
        .and_then(Value::as_array)
        .ok_or("Resource not returned by tool")?;
    let mut contents = Vec::new();
    for block in blocks.iter().filter(|block| {
        block["type"] == "resource"
            && block.pointer("/resource/uri").and_then(Value::as_str) == Some(original_uri)
    }) {
        let mut resource = block["resource"].clone();
        resource["uri"] = json!(requested_uri);
        contents.push(resource);
    }
    if contents.is_empty() {
        return Err("Resource not returned by tool");
    }
    Ok(json!({"contents":contents}))
}

pub(super) async fn read(
    request: virtual_ui::McpCallRequest<'_>,
) -> Result<VirtualToolOutcome, ApiError> {
    let Some(uri) = request.arguments.get("uri").and_then(Value::as_str) else {
        return Ok(VirtualToolOutcome::invalid("Invalid resource URI"));
    };
    let reference = match ResourceReference::decode(uri) {
        Ok(reference) => reference,
        Err(message) => return Ok(VirtualToolOutcome::invalid(message)),
    };
    if reference.user_id != request.actor.user_id
        || reference.workspace_id != request.actor.current_workspace_id
    {
        return Ok(VirtualToolOutcome::invalid("Resource not visible"));
    }
    let Some((instance, tool)) =
        virtual_ui::visible_tool(request.catalog, request.scope, &reference.tool_id, false)
    else {
        return Ok(VirtualToolOutcome::invalid("Resource not visible"));
    };
    if instance.instance_id != reference.instance_id
        || instance.workspace_id != reference.workspace_id
        || tool.workspace_id != reference.workspace_id
        || tool.des_id != reference.des_id
        || !matches!(
            tool.execution_target,
            domain::McpToolExecutionTarget::InterfaceWrapper { .. }
        )
    {
        return Ok(VirtualToolOutcome::invalid(
            "Resource reference is no longer current",
        ));
    }
    if !matches!(media::mapping(&tool.output_mapping), Ok(Some(mapping)) if mapping.resource_read) {
        return Ok(VirtualToolOutcome::invalid(
            "Tool does not support resource reads",
        ));
    }
    let arguments = json!({"tool_id":reference.tool_id,"arguments":reference.arguments});
    let outcome = virtual_ui::call_inner(
        virtual_ui::McpCallRequest {
            arguments: &arguments,
            ..request
        },
        Some(uri),
    )
    .await?;
    match outcome {
        VirtualToolOutcome::Success(result) => {
            match read_result(&result, &reference.resource_uri, uri) {
                Ok(result) => Ok(VirtualToolOutcome::Success(result)),
                Err(message) => Ok(VirtualToolOutcome::invalid(message)),
            }
        }
        error => Ok(error),
    }
}

#[cfg(test)]
#[path = "_tests/resources.rs"]
mod tests;

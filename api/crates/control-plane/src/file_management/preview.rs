use std::collections::HashMap;

use crate::{errors::ControlPlaneError, ports::FileManagementRepository};
use anyhow::Result;
use async_trait::async_trait;
use domain::file_management::{
    file_preview_url, local_image_preview_enabled, local_image_preview_mime,
};
use runtime_core::{
    model_metadata::ModelMetadata, runtime_engine::computed_fields::RuntimeComputedFieldResolver,
};
use serde_json::Value;
use uuid::Uuid;

pub struct FilePreviewProjector<R> {
    repository: R,
}
impl<R> FilePreviewProjector<R> {
    pub fn new(repository: R) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<R: FileManagementRepository> RuntimeComputedFieldResolver for FilePreviewProjector<R> {
    async fn project(&self, metadata: &ModelMetadata, records: &mut [Value]) -> Result<()> {
        let table = self
            .repository
            .find_file_table_by_model_id(metadata.model_id)
            .await?
            .filter(|t| t.status == "active")
            .ok_or(ControlPlaneError::NotFound("file_table"))?;
        // One storage lookup per distinct ID in the page, never per record.
        let mut policies = HashMap::new();
        for record in records {
            let mut preview = Value::Null;
            if record
                .get("mimetype")
                .and_then(Value::as_str)
                .and_then(local_image_preview_mime)
                .is_some()
            {
                if let Some(storage_id) = record
                    .get("storage_id")
                    .and_then(Value::as_str)
                    .and_then(|s| Uuid::parse_str(s).ok())
                {
                    if let std::collections::hash_map::Entry::Vacant(entry) =
                        policies.entry(storage_id)
                    {
                        let enabled = self
                            .repository
                            .get_file_storage(storage_id)
                            .await?
                            .as_ref()
                            .is_some_and(local_image_preview_enabled);
                        entry.insert(enabled);
                    }
                    if policies[&storage_id] {
                        if let Some(id) = record
                            .get("id")
                            .and_then(Value::as_str)
                            .and_then(|s| Uuid::parse_str(s).ok())
                        {
                            preview = Value::String(file_preview_url(table.id, id));
                        }
                    }
                }
            }
            if let Some(object) = record.as_object_mut() {
                object.insert("preview_url".into(), preview);
            }
        }
        Ok(())
    }
}

pub(super) fn normalize_preview_policy(mut rules: Value, driver_type: &str) -> Result<Value> {
    let object = rules
        .as_object_mut()
        .ok_or(ControlPlaneError::InvalidInput("rule_json"))?;
    if let Some(value) = object.get("image_preview_enabled") {
        if !value.is_boolean() {
            return Err(ControlPlaneError::InvalidInput("image_preview_enabled").into());
        }
    }
    if driver_type == "local" {
        object
            .entry("image_preview_enabled")
            .or_insert(Value::Bool(true));
    }
    Ok(rules)
}

#[cfg(test)]
#[path = "_tests/preview_policy.rs"]
mod tests;

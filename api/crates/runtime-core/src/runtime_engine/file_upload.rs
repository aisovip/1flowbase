use super::*;

/// Host-only upload command. It is never deserialized from a generic CRUD request.
pub struct RuntimeUploadedFileInput {
    pub actor: domain::ActorContext,
    pub model_code: String,
    pub metadata: domain::file_management::UploadedFileMetadata,
    pub scope_grant: Option<RuntimeScopeGrant>,
}

impl RuntimeEngine {
    pub async fn create_uploaded_file_record(
        &self,
        input: RuntimeUploadedFileInput,
    ) -> Result<Value> {
        let metadata =
            self.load_available_metadata(&input.model_code, input.actor.current_workspace_id)?;
        // Registration and storage selection are checked by the host FileUploadService.
        // Generic record CRUD cannot construct this command (including its server-generated id).
        if metadata.source_kind != domain::DataModelSourceKind::MainSource {
            return Err(anyhow!("file_upload_requires_main_source"));
        }
        if !domain::file_management::is_file_data_model_template(
            &metadata.template_provider,
            &metadata.template_code,
            &metadata.template_version,
        ) {
            ensure_runtime_record_action_allowed(&metadata, RuntimeDataAction::Create)?;
        }
        let access_scope = resolve_access_scope(
            &input.actor,
            RuntimeDataAction::Create,
            metadata.model_id,
            input.scope_grant.as_ref(),
        )?;
        let scope_id = access_scope
            .scope_id
            .unwrap_or(input.actor.current_workspace_id);
        let payload = self
            .default_value_resolver
            .apply(
                input.actor.user_id,
                &input.model_code,
                serde_json::to_value(input.metadata)?,
            )
            .await?;
        let template = self.resolve_template(&metadata)?;
        ensure_create_api_required_fields(&metadata, &template, &payload)?;
        self.validator
            .validate(input.actor.user_id, &input.model_code, &payload)
            .await?;
        // Resolve fallible computed reads before committing the durable record.
        let mut projected = payload.clone();
        self.project_computed_fields(&metadata, std::slice::from_mut(&mut projected))
            .await?;
        let mut record = self
            .records
            .create_record(&metadata, input.actor.user_id, scope_id, payload)
            .await?;
        if let Some(value) = projected.get("preview_url") {
            record
                .as_object_mut()
                .ok_or_else(|| anyhow!("file record must be an object"))?
                .insert("preview_url".into(), value.clone());
        }
        Ok(record)
    }
}

//! Import orchestration reuses the single upload owner and current contribution authority.
use super::{FileUploadService, FileUploadTarget, UploadFileCommand};
use control_plane_contracts::ports::{
    ContributionAuthorityLease, PluginContributionAuthorityRepository,
};
use extension_contracts::{FileImportError, FileImportResponse};
use runtime_core::runtime_backend::FileImportBinding;
use std::sync::Arc;

pub struct ImportedResourceFile {
    pub bytes: Vec<u8>,
    pub filename: String,
    pub mime_type: Option<String>,
}

/// The adapter releases the pre-read lease before canonical replay (which has its own admission),
/// then reacquires current authority through the upload. No lock is recursively reacquired.
pub async fn authorize_resource_import(
    authority: &dyn PluginContributionAuthorityRepository,
    binding: &FileImportBinding,
) -> Result<Box<dyn ContributionAuthorityLease>, FileImportError> {
    authorize_file_service(authority, binding, "file.import.resource").await
}

pub async fn authorize_file_service(
    authority: &dyn PluginContributionAuthorityRepository,
    binding: &FileImportBinding,
    permission: &str,
) -> Result<Box<dyn ContributionAuthorityLease>, FileImportError> {
    let denied = || FileImportError::new("file_import_authority_denied");
    let installation_id =
        uuid::Uuid::parse_str(binding.subject.installation_id().as_str()).map_err(|_| denied())?;
    let scope_id =
        uuid::Uuid::parse_str(binding.subject.workspace_id().as_str()).map_err(|_| denied())?;
    if scope_id != domain::SYSTEM_SCOPE_ID {
        return Err(denied());
    }
    let lease = authority
        .lock_contribution_authority(&binding.subject)
        .await
        .map_err(|_| denied())?;
    validate_file_service_authority(
        binding,
        permission,
        lease.installation(installation_id).ok_or_else(denied)?,
        lease.snapshot(),
    )?;
    Ok(lease)
}

pub fn validate_resource_import_authority(
    binding: &FileImportBinding,
    installation: &domain::PluginInstallationRecord,
    snapshot: &domain::PluginContributionAuthoritySnapshot,
) -> Result<(), FileImportError> {
    validate_file_service_authority(binding, "file.import.resource", installation, snapshot)
}

fn validate_file_service_authority(
    binding: &FileImportBinding,
    permission: &str,
    installation: &domain::PluginInstallationRecord,
    snapshot: &domain::PluginContributionAuthoritySnapshot,
) -> Result<(), FileImportError> {
    let denied = || FileImportError::new("file_import_authority_denied");
    if installation.id.to_string() != binding.subject.installation_id().as_str()
        || installation.desired_state != domain::PluginDesiredState::ActiveRequested
        || installation.organization != binding.publisher_namespace
        || installation.provider_code != binding.plugin_code
        || installation.plugin_version != binding.plugin_version
        || domain::managed_installation_scope(installation, domain::DEFAULT_SCOPE_ID)
            != domain::SYSTEM_SCOPE_ID
        || binding.subject.workspace_id().as_str() != domain::SYSTEM_SCOPE_ID.to_string()
    {
        return Err(denied());
    }
    let managed: plugin_framework::ManagedManifest =
        serde_json::from_value(installation.metadata_json["managed"].clone())
            .map_err(|_| denied())?;
    let contribution = managed
        .module
        .contributions
        .iter()
        .find(|c| c.contribution_id.as_str() == binding.subject.contribution_id().as_str())
        .ok_or_else(denied)?;
    if contribution.point_id.as_str() != plugin_framework::MANAGED_SERVICE_POINT
        || !contribution
            .required_permissions
            .iter()
            .any(|p| p.as_str() == permission)
    {
        return Err(denied());
    }
    crate::plugin_management::HostContributionGrantPolicy::root_composition()
        .effective_permissions(
            installation,
            domain::SYSTEM_SCOPE_ID,
            contribution,
            snapshot,
        )
        .map_err(|_| denied())?;
    Ok(())
}

pub struct ResourceImportService<R> {
    upload: FileUploadService<R>,
    authority: Arc<dyn PluginContributionAuthorityRepository>,
}
impl<R> ResourceImportService<R>
where
    R: crate::ports::FileManagementRepository + crate::ports::ModelDefinitionRepository + Clone,
{
    pub fn new(
        upload: FileUploadService<R>,
        authority: Arc<dyn PluginContributionAuthorityRepository>,
    ) -> Self {
        Self { upload, authority }
    }
    pub async fn import(
        &self,
        binding: &FileImportBinding,
        actor: domain::ActorContext,
        file_table_id: uuid::Uuid,
        source: ImportedResourceFile,
    ) -> Result<FileImportResponse, FileImportError> {
        if actor.user_id != binding.caller.actor.user_id
            || actor.tenant_id != binding.caller.actor.tenant_id
            || actor.current_workspace_id != binding.caller.actor.current_workspace_id
        {
            return Err(FileImportError::new("file_import_caller_invalid"));
        }
        let lease = authorize_resource_import(self.authority.as_ref(), binding).await?;
        let result = self
            .upload
            .upload(UploadFileCommand {
                actor,
                target: FileUploadTarget::Table(file_table_id),
                original_filename: source.filename,
                content_type: source.mime_type,
                bytes: source.bytes,
            })
            .await
            .map_err(|error| {
                if error.is::<runtime_core::runtime_acl::RuntimeAclError>()
                    || matches!(
                        error.downcast_ref::<crate::errors::ControlPlaneError>(),
                        Some(crate::errors::ControlPlaneError::PermissionDenied(_))
                    )
                {
                    FileImportError::new("file_import_target_permission_denied")
                } else {
                    FileImportError::new("file_import_upload_failed")
                }
            });
        // A failed upload remains a failure; authority is held across actual target effects.
        lease
            .release()
            .await
            .map_err(|_| FileImportError::new("file_import_authority_unavailable"))?;
        let uploaded = result?;
        Ok(FileImportResponse {
            file_table_id: uploaded.file_table_id.to_string(),
            storage_id: uploaded.storage_id.to_string(),
            record: uploaded.record,
        })
    }
}

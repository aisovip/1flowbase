use super::file_management_upload_tests::{
    actor_in_workspace, model_definition, runtime_engine_and_records_for_model, scope_grant,
    temp_root,
};
use crate::{
    _tests::support::MemoryFileManagementRepository,
    file_management::{
        authorize_resource_import, FileUploadService, ImportedResourceFile, ResourceImportService,
    },
};
use domain::{
    ActorContext, ContributionAuthorizationStatus, ContributionResourceScope,
    PluginContributionAuthoritySnapshot, PluginDesiredState, PluginInstallationRecord,
};
use runtime_core::runtime_backend::FileImportBinding;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;
#[path = "authority.rs"]
mod authority;

#[tokio::test]
async fn import_authority_is_explicit_live_and_identity_bound() {
    let actor = actor_in_workspace(Uuid::now_v7());
    let (authority, binding) = authority::fixture(actor);
    let lease = authorize_resource_import(authority.as_ref(), &binding)
        .await
        .unwrap();
    lease.release().await.unwrap();
    for case in 0..5 {
        let (authority, mut binding) = authority::fixture(binding.caller.actor.clone());
        {
            let mut state = authority.state.lock().await;
            match case {
                0 => {
                    state.snapshot.authorizations[1].status =
                        ContributionAuthorizationStatus::Revoked
                }
                1 => state.installation.desired_state = PluginDesiredState::Disabled,
                2 => {
                    state.installation.metadata_json["managed"]["module"]["contributions"][0]
                        ["required_permissions"] = json!(["service.execute"])
                }
                3 => binding.plugin_version = "foreign".into(),
                _ => state.snapshot.authorizations[1].permission_contract_id = "foreign".into(),
            }
        }
        assert!(
            matches!(authorize_resource_import(authority.as_ref(), &binding).await, Err(e) if e.code == "file_import_authority_denied"),
            "case {case}"
        );
    }
}

#[tokio::test]
async fn resource_import_reuses_upload_acl_and_preserves_full_object_and_metadata() {
    let workspace = Uuid::now_v7();
    let actor = actor_in_workspace(workspace);
    let (authority, binding) = authority::fixture(actor.clone());
    let repository = MemoryFileManagementRepository::new(actor.clone());
    let storage_id = Uuid::now_v7();
    let table = Uuid::now_v7();
    let model_id = Uuid::now_v7();
    let model = model_definition(
        model_id,
        domain::DataModelScopeKind::System,
        domain::SYSTEM_SCOPE_ID,
        "imported_files",
    );
    let root = temp_root("resource-import");
    let now = time::OffsetDateTime::now_utc();
    repository
        .insert_file_storage(domain::FileStorageRecord {
            id: storage_id,
            code: "local_default".into(),
            title: "Local".into(),
            driver_type: "local".into(),
            enabled: true,
            is_default: true,
            config_json: json!({"root_path":root.display().to_string(),"public_base_url":null}),
            rule_json: json!({}),
            health_status: domain::FileStorageHealthStatus::Ready,
            last_health_error: None,
            created_by: actor.user_id,
            updated_by: actor.user_id,
            created_at: now,
            updated_at: now,
        })
        .await;
    repository.insert_model_definition(model.clone());
    repository.insert_scope_grant(scope_grant(model_id, workspace));
    repository
        .insert_file_table(domain::FileTableRecord {
            id: table,
            code: "imported_files".into(),
            title: "Imported".into(),
            scope_kind: domain::FileTableScopeKind::Workspace,
            scope_id: workspace,
            model_definition_id: model_id,
            bound_storage_id: Some(storage_id),
            is_builtin: false,
            is_default: false,
            status: "active".into(),
            created_by: actor.user_id,
            updated_by: actor.user_id,
            created_at: now,
            updated_at: now,
        })
        .await;
    let registry = Arc::new(storage_object::builtin_driver_registry());
    let (engine, records) = runtime_engine_and_records_for_model(&model);
    let service = ResourceImportService::new(
        FileUploadService::new(repository.clone(), registry.clone(), engine),
        authority,
    );
    let bytes: Vec<u8> = (0..700_001).map(|n| (n % 251) as u8).collect();
    let source = || ImportedResourceFile {
        bytes: bytes.clone(),
        filename: "original.bin".into(),
        mime_type: Some("application/octet-stream".into()),
    };
    let imported = service
        .import(&binding, actor.clone(), table, source())
        .await
        .unwrap();
    assert_eq!(imported.file_table_id, table.to_string());
    assert_eq!(imported.storage_id, storage_id.to_string());
    assert_eq!(imported.record["filename"], "original.bin");
    assert_eq!(imported.record["mimetype"], "application/octet-stream");
    assert_eq!(imported.record["size"], 700001);
    assert_eq!(imported.record["created_by"], actor.user_id.to_string());
    let stored = registry
        .get("local")
        .unwrap()
        .open_read(storage_object::OpenReadInput {
            config_json: &json!({"root_path":root.display().to_string(),"public_base_url":null}),
            object_path: imported.record["path"].as_str().unwrap(),
        })
        .await
        .unwrap();
    assert_eq!(stored.bytes, bytes);
    let object_directory = root
        .join(imported.record["path"].as_str().unwrap())
        .parent()
        .unwrap()
        .to_path_buf();
    let objects_before = std::fs::read_dir(&object_directory).unwrap().count();
    assert_eq!(records.record_count(&model.code), 1);
    repository.set_scope_grant_enabled(model_id, false);
    let result = service.import(&binding, actor, table, source()).await;
    assert!(matches!(result, Err(e) if e.code == "file_import_target_permission_denied"));
    assert_eq!(
        std::fs::read_dir(&object_directory).unwrap().count(),
        objects_before,
        "ACL failure must not create an orphan object"
    );
    assert_eq!(
        records.record_count(&model.code),
        1,
        "ACL failure must not create a file record"
    );
    let _ = std::fs::remove_dir_all(root);
}

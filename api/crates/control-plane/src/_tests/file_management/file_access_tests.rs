use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use anyhow::Result;
use async_trait::async_trait;
use control_plane::_tests::support::MemoryFileManagementRepository;
use control_plane::{
    file_management::{
        FileAccessKey, FileAccessService, FileUploadService, FileUploadTarget, UploadFileCommand,
    },
    ports::EphemeralKvStore,
};
use domain::{
    DataModelScopeKind, FileStorageHealthStatus, FileStorageRecord, FileTableRecord,
    FileTableScopeKind, SYSTEM_SCOPE_ID,
};
use serde_json::{json, Value};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use super::file_management_upload_tests::{
    actor_in_workspace, model_definition, runtime_engine_for_model, scope_grant, temp_root,
};

#[derive(Default)]
struct TestKv {
    entries: Mutex<HashMap<String, Value>>,
    writes: Mutex<Vec<Duration>>,
}

#[async_trait]
impl EphemeralKvStore for TestKv {
    async fn set_json(&self, key: &str, value: Value, ttl: Option<Duration>) -> Result<()> {
        self.entries.lock().unwrap().insert(key.into(), value);
        self.writes.lock().unwrap().push(ttl.unwrap());
        Ok(())
    }
    async fn get_json(&self, key: &str) -> Result<Option<Value>> {
        Ok(self.entries.lock().unwrap().get(key).cloned())
    }
    async fn delete(&self, key: &str) -> Result<()> {
        self.entries.lock().unwrap().remove(key);
        Ok(())
    }
    async fn touch(&self, _: &str, _: Duration) -> Result<bool> {
        panic!("file access must not renew TTL")
    }
    async fn set_if_absent_json(&self, _: &str, _: Value, _: Option<Duration>) -> Result<bool> {
        anyhow::bail!("unused")
    }
    async fn increment_counter(&self, _: &str, _: i64, _: Option<Duration>) -> Result<i64> {
        anyhow::bail!("unused")
    }
}

#[tokio::test]
async fn file_access_preserves_stable_url_and_scopes_ephemeral_public_read() {
    let workspace_id = Uuid::now_v7();
    let actor = actor_in_workspace(workspace_id);
    let repository = MemoryFileManagementRepository::new(actor.clone());
    let table_id = Uuid::now_v7();
    let storage_id = Uuid::now_v7();
    let mut model = model_definition(
        Uuid::now_v7(),
        DataModelScopeKind::System,
        SYSTEM_SCOPE_ID,
        "files",
    );
    model.template_code = domain::file_management::FILE_DATA_MODEL_TEMPLATE_CODE.into();
    repository.insert_scope_grant(scope_grant(model.id, workspace_id));
    repository.insert_model_definition(model.clone());
    let root = temp_root("temporary-file-access");
    repository
        .insert_file_storage(FileStorageRecord {
            id: storage_id,
            code: "local".into(),
            title: "Local".into(),
            driver_type: "local".into(),
            enabled: true,
            is_default: true,
            config_json: json!({"root_path":root}),
            rule_json: json!({}),
            health_status: FileStorageHealthStatus::Ready,
            last_health_error: None,
            created_by: actor.user_id,
            updated_by: actor.user_id,
            created_at: OffsetDateTime::now_utc(),
            updated_at: OffsetDateTime::now_utc(),
        })
        .await;
    repository
        .insert_file_table(FileTableRecord {
            id: table_id,
            code: model.code.clone(),
            title: "Files".into(),
            scope_kind: FileTableScopeKind::System,
            scope_id: SYSTEM_SCOPE_ID,
            model_definition_id: model.id,
            bound_storage_id: None,
            is_builtin: false,
            is_default: false,
            status: "active".into(),
            created_by: actor.user_id,
            updated_by: actor.user_id,
            created_at: OffsetDateTime::now_utc(),
            updated_at: OffsetDateTime::now_utc(),
        })
        .await;
    let engine = runtime_engine_for_model(&model);
    let drivers = Arc::new(storage_object::builtin_driver_registry());
    let upload = FileUploadService::new(repository.clone(), drivers.clone(), engine.clone())
        .upload(UploadFileCommand {
            actor: actor.clone(),
            target: FileUploadTarget::Table(table_id),
            original_filename: format!("{}.md", "名".repeat(300)),
            content_type: Some("text/markdown".into()),
            bytes: b"file contents".to_vec(),
        })
        .await
        .unwrap();
    let id = Uuid::parse_str(upload.record["id"].as_str().unwrap()).unwrap();
    assert_eq!(
        upload.record["url"],
        domain::file_management::file_content_url(table_id, id)
    );
    assert!(upload.record["filename"].as_str().unwrap().len() > 255);
    repository.insert_file_record(table_id, upload.record.clone());
    let kv = Arc::new(TestKv::default());
    let service = FileAccessService::new(repository.clone(), drivers, engine, kv.clone());
    let link = service
        .create_temporary_link(actor.clone(), table_id, id, 600)
        .await
        .unwrap();
    assert!(link.url.starts_with("/api/public/files/content?token="));
    let token = link.url.split("token=").nth(1).unwrap();
    let mut content = service
        .read_temporary(FileAccessKey::from_token(token).unwrap())
        .await
        .unwrap();
    let mut bytes = Vec::new();
    tokio::io::AsyncReadExt::read_to_end(content.reader.get_mut().unwrap(), &mut bytes)
        .await
        .unwrap();
    assert_eq!(bytes, b"file contents");
    assert_eq!(
        kv.writes.lock().unwrap().as_slice(),
        &[Duration::seconds(600)]
    );
    assert_eq!(
        service
            .read(actor.clone(), table_id, id)
            .await
            .unwrap()
            .filename,
        upload.record["filename"].as_str().unwrap()
    );

    let invalid = FileAccessKey::from_token(&"A".repeat(43)).unwrap();
    assert!(service
        .read_temporary(invalid)
        .await
        .err()
        .unwrap()
        .to_string()
        .contains("temporary_file_access_invalid"));
    assert!(FileAccessKey::from_token("malformed").is_err());
    assert!(service
        .create_temporary_link(actor.clone(), table_id, id, 0)
        .await
        .is_err());
    assert!(service
        .create_temporary_link(actor.clone(), table_id, id, i64::MAX)
        .await
        .is_err());
    let unauthorized = domain::ActorContext::scoped(
        Uuid::now_v7(),
        Uuid::now_v7(),
        "reader",
        Vec::<String>::new(),
    );
    assert!(service
        .create_temporary_link(unauthorized, table_id, id, 60)
        .await
        .is_err());
    assert_eq!(kv.entries.lock().unwrap().len(), 1);

    let mut changed = upload.record.clone();
    changed["path"] = json!("different-object");
    repository.insert_file_record(table_id, changed);
    assert!(service
        .read_temporary(FileAccessKey::from_token(token).unwrap())
        .await
        .is_err());
    repository.insert_file_record(table_id, upload.record);
    for grant in kv.entries.lock().unwrap().values_mut() {
        grant["expires_at"] = json!("1970-01-01T00:00:00Z");
    }
    assert!(service
        .read_temporary(FileAccessKey::from_token(token).unwrap())
        .await
        .is_err());
    assert!(kv.entries.lock().unwrap().is_empty());
    tokio::fs::remove_dir_all(root).await.unwrap();
}

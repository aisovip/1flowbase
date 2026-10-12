use crate::routes::model_definitions::{CreateModelDefinitionBody, UpdateModelDefinitionBody};

#[test]
fn ac_003_create_accepts_identity_but_update_rejects_template_mutability() {
    let create = serde_json::from_value::<CreateModelDefinitionBody>(serde_json::json!({
        "scope_kind": "workspace",
        "template_provider": "core",
        "template_code": "general",
        "template_version": "v1",
        "code": "orders",
        "title": "Orders",
        "description": null,
        "status": "published"
    }))
    .expect("create contract must carry template identity");
    assert_eq!(create.template_provider, "core");
    assert_eq!(create.template_code, "general");
    assert_eq!(create.template_version, "v1");
    assert!(create.bound_storage_id.is_none());

    let update = serde_json::from_value::<UpdateModelDefinitionBody>(serde_json::json!({
        "title": "Renamed",
        "template_provider": "runtime-extension",
        "template_code": "replacement",
        "template_version": "v2"
    }));
    assert!(
        update.is_err(),
        "update contract must reject template identity"
    );
}

#[test]
fn file_model_create_binding_accepts_omitted_null_or_uuid_and_rejects_bad_uuid() {
    let mut body = serde_json::json!({"scope_kind":"workspace", "template_provider":"core", "template_code":"file", "template_version":"v1", "code":"files", "title":"Files"});
    assert!(
        serde_json::from_value::<CreateModelDefinitionBody>(body.clone())
            .unwrap()
            .bound_storage_id
            .is_none()
    );
    body["bound_storage_id"] = serde_json::Value::Null;
    assert!(
        serde_json::from_value::<CreateModelDefinitionBody>(body.clone())
            .unwrap()
            .bound_storage_id
            .is_none()
    );
    let storage_id = uuid::Uuid::now_v7();
    body["bound_storage_id"] = serde_json::json!(storage_id);
    assert_eq!(
        serde_json::from_value::<CreateModelDefinitionBody>(body.clone())
            .unwrap()
            .bound_storage_id,
        Some(storage_id)
    );
    body["bound_storage_id"] = serde_json::json!("invalid");
    assert!(serde_json::from_value::<CreateModelDefinitionBody>(body).is_err());
}

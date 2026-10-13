use super::*;

fn input(workspace_id: Uuid, actor_user_id: Uuid, code: &str) -> CreateModelDefinitionInput {
    CreateModelDefinitionInput {
        actor_user_id,
        bound_storage_id: None,
        scope_kind: DataModelScopeKind::Workspace,
        scope_id: workspace_id,
        data_source_instance_id: None,
        source_kind: DataModelSourceKind::MainSource,
        external_resource_key: None,
        external_table_id: None,
        external_capability_snapshot: None,
        template_provider: "core".into(),
        template_code: "file".into(),
        template_version: "v1".into(),
        code: code.into(),
        title: "Files".into(),
        description: None,
        status: DataModelStatus::Published,
        protection: DataModelProtection::default(),
    }
}

#[tokio::test]
async fn file_template_creation_atomically_installs_owned_fields_physical_table_and_registration() {
    let pool = isolated_database().await.connect().await.unwrap();
    run_migrations(&pool).await.unwrap();
    let store = PgControlPlaneStore::new(pool);
    let (workspace_id, actor_user_id, _) =
        seed_data_source_workspace(&store, "File Template", "file_fixture").await;
    let model = ModelDefinitionRepository::create_model_definition(
        &store,
        &input(workspace_id, actor_user_id, "workspace_files"),
    )
    .await
    .unwrap();
    for (code, _, _, _) in domain::file_management::FILE_METADATA_FIELDS {
        let field = model
            .fields
            .iter()
            .find(|field| field.code == *code)
            .unwrap();
        assert!(field.is_system && !field.is_writable && !field.api_required);
        let computed = field.field_kind == domain::ModelFieldKind::FilePreviewUrl;
        assert_eq!(
            field.physical_column_name,
            if computed { "" } else { *code }
        );
        let physical: bool = sqlx::query_scalar("select exists(select 1 from information_schema.columns where table_schema=current_schema() and table_name=$1 and column_name=$2)")
            .bind(&model.physical_table_name).bind(*code).fetch_one(store.pool()).await.unwrap();
        assert!(
            physical != computed,
            "only physical file fields create a column: {code}"
        );
    }
    let table = control_plane_contracts::ports::FileManagementRepository::find_file_table_by_code(
        &store,
        &model.code,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(table.model_definition_id, model.id);
    assert_eq!(table.scope_id, workspace_id);
    assert!(table.bound_storage_id.is_none());
    assert!(!table.is_builtin && !table.is_default);

    let table_count_before: i64 =
        sqlx::query_scalar("select count(*) from pg_tables where schemaname=current_schema()")
            .fetch_one(store.pool())
            .await
            .unwrap();
    let mut invalid = input(workspace_id, actor_user_id, "failed_file_creation");
    invalid.bound_storage_id = Some(Uuid::now_v7());
    assert!(
        ModelDefinitionRepository::create_model_definition(&store, &invalid)
            .await
            .is_err(),
        "an unknown storage must abort the transaction"
    );
    let partial_models: i64 = sqlx::query_scalar(
        "select count(*) from model_definitions where code='failed_file_creation'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(
        partial_models, 0,
        "rollback must not recreate a broken file model"
    );
    assert!(
        control_plane_contracts::ports::FileManagementRepository::find_file_table_by_code(
            &store,
            "failed_file_creation"
        )
        .await
        .unwrap()
        .is_none()
    );
    let disabled_storage =
        control_plane_contracts::ports::FileManagementRepository::create_file_storage(
            &store,
            &control_plane_contracts::ports::CreateFileStorageInput {
                storage_id: Uuid::now_v7(),
                actor_user_id,
                code: "disabled_file_fixture".into(),
                title: "Disabled".into(),
                driver_type: "local".into(),
                enabled: false,
                is_default: false,
                config_json: serde_json::json!({}),
                rule_json: serde_json::json!({}),
            },
        )
        .await
        .unwrap();
    let mut disabled = input(workspace_id, actor_user_id, "disabled_file_creation");
    disabled.bound_storage_id = Some(disabled_storage.id);
    let error = ModelDefinitionRepository::create_model_definition(&store, &disabled)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("file_storage_disabled"));
    let disabled_models: i64 = sqlx::query_scalar(
        "select count(*) from model_definitions where code='disabled_file_creation'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(disabled_models, 0);
    let table_count_after: i64 =
        sqlx::query_scalar("select count(*) from pg_tables where schemaname=current_schema()")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(
        table_count_after, table_count_before,
        "file DDL must roll back with registration"
    );
    store.pool().close().await;
}

use anyhow::Result;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

pub(super) fn matches(model: &domain::ModelDefinitionRecord) -> bool {
    domain::file_management::is_file_data_model_template(
        &model.template_provider,
        &model.template_code,
        &model.template_version,
    )
}

pub(super) fn field_records(
    model: &domain::ModelDefinitionRecord,
) -> Vec<domain::ModelFieldRecord> {
    if !matches(model) {
        return Vec::new();
    }
    domain::file_management::FILE_METADATA_FIELDS
        .iter()
        .enumerate()
        .map(
            |(offset, (code, title, field_kind, is_required))| domain::ModelFieldRecord {
                id: Uuid::now_v7(),
                data_model_id: model.id,
                code: (*code).to_owned(),
                title: (*title).to_owned(),
                description: None,
                physical_column_name: if *field_kind == domain::ModelFieldKind::FilePreviewUrl {
                    String::new()
                } else {
                    (*code).to_owned()
                },
                external_field_key: None,
                field_kind: *field_kind,
                is_system: true,
                is_writable: false,
                is_required: *is_required,
                api_required: false,
                is_unique: false,
                default_value: None,
                display_interface: None,
                display_options: serde_json::json!({}),
                relation_target_model_id: None,
                relation_options: serde_json::json!({}),
                sort_order: 6 + offset as i32,
                availability_status: domain::MetadataAvailabilityStatus::Available,
            },
        )
        .collect()
}

pub(super) async fn provision(
    tx: &mut Transaction<'_, Postgres>,
    model: &domain::ModelDefinitionRecord,
    actor_user_id: Uuid,
    bound_storage_id: Option<Uuid>,
) -> Result<()> {
    if !matches(model) {
        return Ok(());
    }
    if let Some(storage_id) = bound_storage_id {
        let enabled = sqlx::query_scalar::<_, bool>(
            "select enabled from file_storages where id=$1 for share",
        )
        .bind(storage_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(control_plane_contracts::ControlPlaneContractError::NotFound("file_storage"))?;
        if !enabled {
            return Err(
                control_plane_contracts::ControlPlaneContractError::Conflict(
                    "file_storage_disabled",
                )
                .into(),
            );
        }
    }
    for field in field_records(model) {
        if field.field_kind != domain::ModelFieldKind::FilePreviewUrl {
            crate::physical_schema_repository::add_scalar_column(tx, model, &field).await?;
        }
    }
    sqlx::query("insert into file_tables (id,code,title,scope_kind,scope_id,model_definition_id,bound_storage_id,is_builtin,is_default,status,created_by,updated_by) values ($1,$2,$3,$4,$5,$6,$7,false,false,'active',$8,$8)")
        .bind(Uuid::now_v7()).bind(&model.code).bind(&model.title)
        .bind(model.scope_kind.as_str()).bind(model.scope_id).bind(model.id)
        .bind(bound_storage_id).bind(actor_user_id).execute(&mut **tx).await?;
    Ok(())
}

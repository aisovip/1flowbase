use anyhow::Result;
use sqlx::Row;
use uuid::Uuid;

use crate::repositories::PgControlPlaneStore;

pub(super) async fn get_file_record_for_access(
    store: &PgControlPlaneStore,
    file_table_id: Uuid,
    record_id: Uuid,
    scope_id: Uuid,
) -> Result<Option<serde_json::Value>> {
    let row = sqlx::query(
        "select definitions.physical_table_name from file_tables tables join model_definitions definitions on definitions.id = tables.model_definition_id where tables.id = $1 and tables.status = 'active' and definitions.source_kind = 'main_source' and definitions.data_source_instance_id is null and definitions.status = 'published' and definitions.availability_status = 'available'"
    ).bind(file_table_id).fetch_optional(store.pool()).await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let name: String = row.try_get("physical_table_name")?;
    let table = crate::physical_schema_repository::quote_identifier(&name)?;
    let statement = format!(
        "select to_jsonb(record) as record from {table} record where id = $1 and scope_id = $2"
    );
    let row = sqlx::query(&statement)
        .bind(record_id)
        .bind(scope_id)
        .fetch_optional(store.pool())
        .await?;
    row.map(|row| row.try_get("record"))
        .transpose()
        .map_err(Into::into)
}

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

pub const FILE_DATA_MODEL_TEMPLATE_CODE: &str = "file";
pub const FILE_DATA_MODEL_TEMPLATE_VERSION: &str = "v1";

pub fn is_file_data_model_template(provider: &str, code: &str, version: &str) -> bool {
    provider == crate::CORE_DATA_MODEL_TEMPLATE_PROVIDER
        && code == FILE_DATA_MODEL_TEMPLATE_CODE
        && version == FILE_DATA_MODEL_TEMPLATE_VERSION
}

/// The object-storage service owns these fields. Ordinary record CRUD cannot write them.
pub const FILE_METADATA_FIELDS: &[(&str, &str, crate::ModelFieldKind, bool)] = &[
    ("title", "Title", crate::ModelFieldKind::String, false),
    ("filename", "Filename", crate::ModelFieldKind::String, true),
    ("extname", "Extension", crate::ModelFieldKind::String, false),
    ("size", "Size", crate::ModelFieldKind::Number, true),
    ("mimetype", "MIME Type", crate::ModelFieldKind::String, true),
    ("path", "Storage Path", crate::ModelFieldKind::Text, true),
    ("meta", "Metadata", crate::ModelFieldKind::Json, true),
    ("url", "Access URL", crate::ModelFieldKind::Text, true),
    (
        "storage_id",
        "Storage ID",
        crate::ModelFieldKind::String,
        true,
    ),
];

/// Produced after an object-storage driver successfully writes the object.
#[derive(Debug, Clone, Serialize)]
pub struct UploadedFileMetadata {
    pub id: Uuid,
    pub title: String,
    pub filename: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extname: Option<String>,
    pub size: u64,
    pub mimetype: String,
    pub path: String,
    pub meta: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub storage_id: Uuid,
}

/// Stable same-origin entry point; it never grants anonymous access.
pub fn file_content_url(file_table_id: Uuid, record_id: Uuid) -> String {
    format!("/api/console/files/{file_table_id}/records/{record_id}/content")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStorageHealthStatus {
    Unknown,
    Ready,
    Failed,
}

impl FileStorageHealthStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileTableScopeKind {
    System,
    Workspace,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileStorageRecord {
    pub id: Uuid,
    pub code: String,
    pub title: String,
    pub driver_type: String,
    pub enabled: bool,
    pub is_default: bool,
    pub config_json: serde_json::Value,
    pub rule_json: serde_json::Value,
    pub health_status: FileStorageHealthStatus,
    pub last_health_error: Option<String>,
    pub created_by: Uuid,
    pub updated_by: Uuid,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileTableRecord {
    pub id: Uuid,
    pub code: String,
    pub title: String,
    pub scope_kind: FileTableScopeKind,
    pub scope_id: Uuid,
    pub model_definition_id: Uuid,
    pub bound_storage_id: Option<Uuid>,
    pub is_builtin: bool,
    pub is_default: bool,
    pub status: String,
    pub created_by: Uuid,
    pub updated_by: Uuid,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

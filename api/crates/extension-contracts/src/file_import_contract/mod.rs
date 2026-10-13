//! File import wire contract. Locators grant no authority and workers supply no identity.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileImportRequest {
    pub resource_uri: String,
    pub file_table_id: String,
}
impl FileImportRequest {
    pub fn validate(&self) -> Result<(), FileImportError> {
        if !self.resource_uri.starts_with(ResourceReference::PREFIX)
            || self.file_table_id.parse::<uuid::Uuid>().is_err()
        {
            return Err(FileImportError::new("file_import_request_invalid"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileImportResponse {
    pub file_table_id: String,
    pub storage_id: String,
    pub record: Value,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileImportError {
    pub code: String,
}
impl FileImportError {
    pub fn new(code: &str) -> Self {
        Self { code: code.into() }
    }
}
impl std::fmt::Display for FileImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.code)
    }
}
impl std::error::Error for FileImportError {}

/// Replay identity only; current authorization and transport budgets belong to the Host.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceReference {
    pub v: u8,
    pub user_id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
    pub instance_id: String,
    pub tool_id: String,
    pub des_id: String,
    pub arguments: Value,
    pub resource_uri: String,
}
impl ResourceReference {
    pub const PREFIX: &'static str = "flowbase-resource://read/";
    pub fn encode(&self) -> Result<String, &'static str> {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        self.validate()?;
        Ok(format!(
            "{}{}",
            Self::PREFIX,
            URL_SAFE_NO_PAD
                .encode(serde_json::to_vec(self).map_err(|_| "Invalid resource reference")?)
        ))
    }
    pub fn decode(uri: &str) -> Result<Self, &'static str> {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        let encoded = uri
            .strip_prefix(Self::PREFIX)
            .ok_or("Unknown resource URI")?;
        let bytes = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| "Invalid resource reference")?;
        let reference: Self =
            serde_json::from_slice(&bytes).map_err(|_| "Invalid resource reference")?;
        reference.validate()?;
        Ok(reference)
    }
    fn validate(&self) -> Result<(), &'static str> {
        let bytes = self.resource_uri.as_bytes();
        if self.v != 1
            || self.instance_id.is_empty()
            || self.tool_id.is_empty()
            || self.des_id.is_empty()
            || !self.arguments.is_object()
            || self.resource_uri.starts_with(Self::PREFIX)
            || bytes.iter().any(|b| {
                b.is_ascii_control() || b.is_ascii_whitespace() || b"<>\\\"{}|^`".contains(b)
            })
            || bytes.iter().enumerate().any(|(i, b)| {
                *b == b'%'
                    && !(bytes.get(i + 1).is_some_and(u8::is_ascii_hexdigit)
                        && bytes.get(i + 2).is_some_and(u8::is_ascii_hexdigit))
            })
            || url::Url::parse(&self.resource_uri).is_err()
        {
            return Err("Invalid resource reference");
        }
        Ok(())
    }
}

//! Call-scoped binary reads. Identifiers are selectors, never authority.
use serde::{Deserialize, Serialize};

pub const FILE_READ_CHUNK_BYTES: usize = 64 * 1024;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum FileReadRequest {
    Open {
        file_table_id: String,
        record_id: String,
    },
    Chunk {
        ticket: String,
        offset: u64,
    },
    Close {
        ticket: String,
    },
}
impl FileReadRequest {
    pub fn validate(&self) -> Result<(), FileReadError> {
        let valid = match self {
            Self::Open {
                file_table_id,
                record_id,
            } => {
                file_table_id.parse::<uuid::Uuid>().is_ok()
                    && record_id.parse::<uuid::Uuid>().is_ok()
            }
            Self::Chunk { ticket, .. } | Self::Close { ticket } => {
                ticket.parse::<uuid::Uuid>().is_ok()
            }
        };
        if valid {
            Ok(())
        } else {
            Err(FileReadError::new("file_read_request_invalid"))
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileReadMetadata {
    pub filename: String,
    pub content_type: Option<String>,
    pub content_length: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum FileReadResponse {
    Open {
        ticket: String,
        metadata: FileReadMetadata,
    },
    Chunk {
        ticket: String,
        offset: u64,
        data_base64: String,
        eof: bool,
        sha256: Option<String>,
    },
    Close {
        ticket: String,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileReadError {
    pub code: String,
}
impl FileReadError {
    pub fn new(code: &str) -> Self {
        Self { code: code.into() }
    }
}

impl std::fmt::Display for FileReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.code)
    }
}
impl std::error::Error for FileReadError {}

#[cfg(test)]
mod _tests;

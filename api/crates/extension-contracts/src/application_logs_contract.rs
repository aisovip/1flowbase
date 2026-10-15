//! Source-neutral managed application log exchange. Trusted identity is Host-only.
#[cfg(test)]
#[path = "_tests/exchange_host_services.rs"]
mod exchange_host_services;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ApplicationLogsRequest {
    ListApplications {
        application_types: Vec<String>,
    },
    ListRecords {
        application_id: String,
        started_at_from: Option<String>,
        started_at_to: Option<String>,
        cursor: Option<String>,
        limit: Option<u32>,
    },
    ExportOtlp {
        application_id: String,
        record_ids: Option<Vec<String>>,
        log_conversation_ids: Option<Vec<String>>,
    },
    ImportOtlp {
        application_id: String,
        otlp: Value,
    },
}
impl ApplicationLogsRequest {
    pub fn permission(&self) -> &'static str {
        match self {
            Self::ImportOtlp { .. } => "application.logs.import",
            _ => "application.logs.read",
        }
    }
    pub fn validate(&self) -> Result<(), HostServiceError> {
        if let Self::ExportOtlp {
            record_ids,
            log_conversation_ids,
            ..
        } = self
        {
            match (record_ids, log_conversation_ids) {
                (Some(ids), None) | (None, Some(ids))
                    if !ids.is_empty() && ids.iter().all(|id| !id.is_empty()) =>
                {
                    ()
                }
                _ => return Err(HostServiceError::new("application_logs_selector_invalid")),
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationLogsApplication {
    pub application_id: String,
    pub name: String,
    pub application_type: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationLogsRecord {
    pub record_id: String,
    pub log_conversation_id: Option<String>,
    pub title: String,
    pub started_at: String,
    pub source_kind: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ApplicationLogsResponse {
    Applications {
        items: Vec<ApplicationLogsApplication>,
    },
    Records {
        items: Vec<ApplicationLogsRecord>,
        next_cursor: Option<String>,
    },
    Exported {
        otlp: Value,
        record_ids: Vec<String>,
        trace_ids: Vec<String>,
        exported_spans: usize,
    },
    Imported {
        accepted_events: usize,
        duplicate_events: usize,
        record_ids: Vec<String>,
    },
}
/// Safe codes and structured receipts; never contains transport error strings or credentials.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[error("{code}")]
#[serde(deny_unknown_fields)]
pub struct HostServiceError {
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}
impl HostServiceError {
    pub fn new(code: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            status: None,
            details: None,
        }
    }
}

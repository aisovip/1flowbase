use crate::{MultiplexEmitter, MultiplexError, MultiplexHostService};
use extension_contracts::{FileImportError, FileImportRequest, FileImportResponse};
use serde::Deserialize;

#[derive(Debug, thiserror::Error)]
pub enum FileImportClientError {
    #[error(transparent)]
    Transport(#[from] MultiplexError),
    #[error(transparent)]
    Host(#[from] FileImportError),
    #[error("invalid file_import host response")]
    InvalidResponse,
}
#[derive(Clone)]
pub struct FileImportClient {
    emitter: MultiplexEmitter,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResponseEnvelope {
    result: Option<FileImportResponse>,
    error: Option<FileImportError>,
}
impl FileImportClient {
    pub fn new(emitter: MultiplexEmitter) -> Self {
        Self { emitter }
    }
    async fn execute(
        &self,
        request: FileImportRequest,
    ) -> Result<FileImportResponse, FileImportClientError> {
        request.validate()?;
        let request =
            serde_json::to_value(request).map_err(|_| FileImportClientError::InvalidResponse)?;
        let response = self
            .emitter
            .callback(MultiplexHostService::FileImportV1, request)
            .await?;
        let response: ResponseEnvelope =
            serde_json::from_value(response).map_err(|_| FileImportClientError::InvalidResponse)?;
        match (response.result, response.error) {
            (Some(result), None) => Ok(result),
            (None, Some(error)) => Err(error.into()),
            _ => Err(FileImportClientError::InvalidResponse),
        }
    }
    /// Import a current Host replay locator into a file table using the admitted caller's ACL.
    pub async fn import_resource(
        &self,
        resource_uri: String,
        file_table_id: String,
    ) -> Result<FileImportResponse, FileImportClientError> {
        self.execute(FileImportRequest {
            resource_uri,
            file_table_id,
        })
        .await
    }
}

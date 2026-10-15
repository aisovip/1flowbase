use crate::{MultiplexEmitter, MultiplexError, MultiplexHostService};
use extension_contracts::{ApplicationLogsRequest, ApplicationLogsResponse, HostServiceError};
#[derive(Debug, thiserror::Error)]
pub enum ApplicationLogsClientError {
    #[error(transparent)]
    Transport(#[from] MultiplexError),
    #[error(transparent)]
    Host(#[from] HostServiceError),
    #[error("invalid host service response")]
    InvalidResponse,
}
#[derive(Clone)]
pub struct ApplicationLogsClient {
    emitter: MultiplexEmitter,
}
impl ApplicationLogsClient {
    pub fn new(emitter: MultiplexEmitter) -> Self {
        Self { emitter }
    }
    pub async fn execute(
        &self,
        request: ApplicationLogsRequest,
    ) -> Result<ApplicationLogsResponse, ApplicationLogsClientError> {
        request.validate()?;
        let request = serde_json::to_value(request)
            .map_err(|_| ApplicationLogsClientError::InvalidResponse)?;
        let response = self
            .emitter
            .callback(MultiplexHostService::ApplicationLogsV1, request)
            .await?;
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Envelope {
            result: Option<ApplicationLogsResponse>,
            error: Option<HostServiceError>,
        }
        let response: Envelope = serde_json::from_value(response)
            .map_err(|_| ApplicationLogsClientError::InvalidResponse)?;
        match (response.result, response.error) {
            (Some(result), None) => Ok(result),
            (None, Some(error)) => Err(error.into()),
            _ => Err(ApplicationLogsClientError::InvalidResponse),
        }
    }
}

use crate::{MultiplexEmitter, MultiplexError, MultiplexHostService};
use extension_contracts::{HostServiceError, HttpEgressRequest, HttpEgressResponse};
#[derive(Debug, thiserror::Error)]
pub enum HttpEgressClientError {
    #[error(transparent)]
    Transport(#[from] MultiplexError),
    #[error(transparent)]
    Host(#[from] HostServiceError),
    #[error("invalid host service response")]
    InvalidResponse,
}
#[derive(Clone)]
pub struct HttpEgressClient {
    emitter: MultiplexEmitter,
}
impl HttpEgressClient {
    pub fn new(emitter: MultiplexEmitter) -> Self {
        Self { emitter }
    }
    pub async fn execute(
        &self,
        request: HttpEgressRequest,
    ) -> Result<HttpEgressResponse, HttpEgressClientError> {
        request.validate()?;
        let request =
            serde_json::to_value(request).map_err(|_| HttpEgressClientError::InvalidResponse)?;
        let response = self
            .emitter
            .callback(MultiplexHostService::HttpEgressV1, request)
            .await?;
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Envelope {
            result: Option<HttpEgressResponse>,
            error: Option<HostServiceError>,
        }
        let response: Envelope =
            serde_json::from_value(response).map_err(|_| HttpEgressClientError::InvalidResponse)?;
        match (response.result, response.error) {
            (Some(result), None) => Ok(result),
            (None, Some(error)) => Err(error.into()),
            _ => Err(HttpEgressClientError::InvalidResponse),
        }
    }
}

//! Shared verified binary pump; plugins own only their destination writer.
use crate::{MultiplexEmitter, MultiplexError, MultiplexHostService};
use base64::{engine::general_purpose::STANDARD, Engine};
use extension_contracts::{
    FileReadError, FileReadMetadata, FileReadRequest, FileReadResponse, FILE_READ_CHUNK_BYTES,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncWrite, AsyncWriteExt};

#[derive(Debug, thiserror::Error)]
pub enum FileReadClientError {
    #[error(transparent)]
    Transport(#[from] MultiplexError),
    #[error(transparent)]
    Host(#[from] FileReadError),
    #[error(transparent)]
    Write(#[from] std::io::Error),
    #[error("invalid file_read host response")]
    InvalidResponse,
}
#[derive(Clone)]
pub struct FileReadClient {
    emitter: MultiplexEmitter,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    result: Option<FileReadResponse>,
    error: Option<FileReadError>,
}
impl FileReadClient {
    pub fn new(emitter: MultiplexEmitter) -> Self {
        Self { emitter }
    }
    async fn execute(
        &self,
        request: FileReadRequest,
    ) -> Result<FileReadResponse, FileReadClientError> {
        request.validate()?;
        let value =
            serde_json::to_value(request).map_err(|_| FileReadClientError::InvalidResponse)?;
        let envelope: Envelope = serde_json::from_value(
            self.emitter
                .callback(MultiplexHostService::FileReadV1, value)
                .await?,
        )
        .map_err(|_| FileReadClientError::InvalidResponse)?;
        match (envelope.result, envelope.error) {
            (Some(result), None) => Ok(result),
            (None, Some(error)) => Err(error.into()),
            _ => Err(FileReadClientError::InvalidResponse),
        }
    }
    pub async fn open(
        &self,
        file_table_id: String,
        record_id: String,
    ) -> Result<FileReader, FileReadClientError> {
        match self
            .execute(FileReadRequest::Open {
                file_table_id,
                record_id,
            })
            .await?
        {
            FileReadResponse::Open { ticket, metadata }
                if FileReadRequest::Close {
                    ticket: ticket.clone(),
                }
                .validate()
                .is_ok() =>
            {
                Ok(FileReader {
                    client: self.clone(),
                    ticket,
                    metadata,
                    closed: false,
                    started: false,
                })
            }
            _ => Err(FileReadClientError::InvalidResponse),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferReport {
    pub bytes: u64,
    pub sha256: String,
}
pub struct FileReader {
    client: FileReadClient,
    ticket: String,
    metadata: FileReadMetadata,
    closed: bool,
    started: bool,
}
impl FileReader {
    pub fn metadata(&self) -> &FileReadMetadata {
        &self.metadata
    }
    pub async fn close(&mut self) -> Result<(), FileReadClientError> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        match self
            .client
            .execute(FileReadRequest::Close {
                ticket: self.ticket.clone(),
            })
            .await?
        {
            FileReadResponse::Close { ticket } if ticket == self.ticket => Ok(()),
            _ => Err(FileReadClientError::InvalidResponse),
        }
    }
    /// Pumps once, verifies the complete stream, and closes. Does not shut down the writer.
    pub async fn copy_to<W: AsyncWrite + Unpin>(
        &mut self,
        writer: &mut W,
    ) -> Result<TransferReport, FileReadClientError> {
        if self.closed || self.started {
            return Err(FileReadClientError::InvalidResponse);
        }
        self.started = true;
        let result = self.pump(writer).await;
        let close = self.close().await;
        match result {
            Err(error) => Err(error),
            Ok(report) => {
                close?;
                Ok(report)
            }
        }
    }
    async fn pump<W: AsyncWrite + Unpin>(
        &self,
        writer: &mut W,
    ) -> Result<TransferReport, FileReadClientError> {
        let mut bytes = 0_u64;
        let mut hash = Sha256::new();
        loop {
            let response = self
                .client
                .execute(FileReadRequest::Chunk {
                    ticket: self.ticket.clone(),
                    offset: bytes,
                })
                .await?;
            let FileReadResponse::Chunk {
                ticket,
                offset,
                data_base64,
                eof,
                sha256,
            } = response
            else {
                return Err(FileReadClientError::InvalidResponse);
            };
            if ticket != self.ticket
                || offset != bytes
                || data_base64.len() > FILE_READ_CHUNK_BYTES.div_ceil(3) * 4
            {
                return Err(FileReadClientError::InvalidResponse);
            }
            let chunk = STANDARD
                .decode(data_base64)
                .map_err(|_| FileReadClientError::InvalidResponse)?;
            let total = bytes
                .checked_add(chunk.len() as u64)
                .ok_or(FileReadClientError::InvalidResponse)?;
            if chunk.len() > FILE_READ_CHUNK_BYTES
                || total > self.metadata.content_length
                || (!eof && (chunk.is_empty() || sha256.is_some()))
                || (eof && total != self.metadata.content_length)
            {
                return Err(FileReadClientError::InvalidResponse);
            }
            hash.update(&chunk);
            writer.write_all(&chunk).await?;
            bytes = total;
            if eof {
                let actual = format!("{:x}", hash.finalize());
                if sha256.as_deref() != Some(&actual) {
                    return Err(FileReadClientError::InvalidResponse);
                }
                return Ok(TransferReport {
                    bytes,
                    sha256: actual,
                });
            }
        }
    }
}

//! Actor-authorized source import. Console has no Application API key identity.
use super::*;
use crate::application::{ApplicationNonCrudConsoleOperation, ApplicationService};
use crate::errors::ControlPlaneError;
use std::collections::BTreeSet;

#[derive(Debug, thiserror::Error)]
#[error("agent_logs_partial_import")]
pub struct AgentLogsPartialImportError {
    pub accepted_events: usize,
    pub duplicate_events: usize,
    pub committed_records: usize,
}

impl<R> AgentLogsService<R>
where
    R: crate::ports::ApplicationRepository
        + BillingRepository
        + OrchestrationRuntimeRepository
        + Clone,
{
    /// Import fully gathered and mapped source batches with explicit Console provenance.
    /// Each source batch remains atomic. A later failure reports partial commit;
    /// retrying unchanged source identities is safe and does not re-meter facts.
    pub async fn import_console_batches(
        &self,
        actor: &domain::ActorContext,
        application_id: Uuid,
        operation: ApplicationNonCrudConsoleOperation,
        batches: Vec<AgentLogsBatch>,
    ) -> Result<AgentLogsReceipt> {
        if !matches!(
            operation,
            ApplicationNonCrudConsoleOperation::LogsOtlpImport
                | ApplicationNonCrudConsoleOperation::LogsLangfuseImport
        ) {
            return Err(ControlPlaneError::InvalidInput("agent_logs_import_operation").into());
        }
        let application = ApplicationService::new(self.repository.clone())
            .load_application_for_non_crud_console_operation_for_actor(
                actor,
                application_id,
                operation,
            )
            .await?;
        if application.application_type != domain::ApplicationType::AgentLogs {
            return Err(ControlPlaneError::InvalidInput("agent_logs_application_type").into());
        }
        if application.sections.api.api_capability_status != "enabled" {
            return Err(
                ControlPlaneError::PermissionDenied("agent_logs_ingestion_disabled").into(),
            );
        }
        for batch in &batches {
            batch
                .validate()
                .map_err(|_| ControlPlaneError::InvalidInput("agent_logs_batch"))?;
        }
        let mut receipt = AgentLogsReceipt {
            accepted_events: 0,
            duplicate_events: 0,
            record_ids: vec![],
        };
        let mut records = BTreeSet::new();
        for batch in batches {
            match self
                .ingest_batch(application_id, application.workspace_id, None, batch)
                .await
            {
                Ok(batch) => {
                    receipt.accepted_events += batch.accepted_events;
                    receipt.duplicate_events += batch.duplicate_events;
                    records.extend(batch.record_ids);
                }
                Err(error) if records.is_empty() => return Err(error),
                Err(_) => {
                    return Err(AgentLogsPartialImportError {
                        accepted_events: receipt.accepted_events,
                        duplicate_events: receipt.duplicate_events,
                        committed_records: records.len(),
                    }
                    .into())
                }
            }
        }
        receipt.record_ids = records.into_iter().collect();
        Ok(receipt)
    }
}

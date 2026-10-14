//! Actor-authorized Agent Logs file and Langfuse exchange through one Console owner.
use crate::{
    agent_logs_protocol as protocol,
    app_state::ApiState,
    error_response::ApiError,
    network_egress_client::{NetworkEgressHttpClientResolver, NetworkEgressHttpRequestScope},
    response::ApiSuccess,
    routes::console_interface::{
        self, ConsoleInterfaceDeclaration, ConsoleInterfaceFuture, ConsoleInterfacePort,
        ConsoleInterfaceTargetError,
    },
};
use control_plane::{
    agent_logs::AgentLogsService,
    application::{ApplicationNonCrudConsoleOperation, ApplicationService},
    errors::ControlPlaneError,
};
use control_plane_contracts::ports::{
    AgentLogsExportSelector, AgentLogsReceipt, OrchestrationRuntimeRepository,
};
use interface_runtime::{InterfaceContract, UserPrincipal};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeSet, sync::Arc};
use storage_durable_postgres::MainDurableStore;
use uuid::Uuid;

mod interface;
mod projection;
mod remote;
mod routes;

pub(crate) use interface::{compile_registry, OPERATIONS};
pub(crate) use routes::*;

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct OtlpExportBody {
    pub record_ids: Option<Vec<Uuid>>,
    pub log_conversation_ids: Option<Vec<Uuid>>,
}
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct OtlpImportBody {
    pub otlp: Value,
}
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct LangfuseExportBody {
    pub record_ids: Option<Vec<Uuid>>,
    pub log_conversation_ids: Option<Vec<Uuid>>,
    pub base_url: String,
    pub public_key: String,
    pub secret_key: String,
}
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct LangfuseImportBody {
    pub trace_ids: Vec<String>,
    pub base_url: String,
    pub public_key: String,
    pub secret_key: String,
}
#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct LangfuseExportReceipt {
    pub exported_records: usize,
    pub exported_spans: usize,
    pub trace_ids: Vec<String>,
}
#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ImportReceipt {
    pub accepted_events: usize,
    pub duplicate_events: usize,
    pub record_ids: Vec<Uuid>,
}
impl From<AgentLogsReceipt> for ImportReceipt {
    fn from(receipt: AgentLogsReceipt) -> Self {
        Self {
            accepted_events: receipt.accepted_events,
            duplicate_events: receipt.duplicate_events,
            record_ids: receipt.record_ids,
        }
    }
}

pub(crate) enum ExchangeInput {
    OtlpExport {
        application_id: Uuid,
        body: OtlpExportBody,
    },
    OtlpImport {
        application_id: Uuid,
        body: OtlpImportBody,
    },
    LangfuseExport {
        application_id: Uuid,
        body: LangfuseExportBody,
    },
    LangfuseImport {
        application_id: Uuid,
        body: LangfuseImportBody,
    },
}
impl ExchangeInput {
    fn identity(&self) -> (Uuid, ApplicationNonCrudConsoleOperation) {
        match self {
            Self::OtlpExport { application_id, .. } => (
                *application_id,
                ApplicationNonCrudConsoleOperation::LogsOtlpExport,
            ),
            Self::OtlpImport { application_id, .. } => (
                *application_id,
                ApplicationNonCrudConsoleOperation::LogsOtlpImport,
            ),
            Self::LangfuseExport { application_id, .. } => (
                *application_id,
                ApplicationNonCrudConsoleOperation::LogsLangfuseExport,
            ),
            Self::LangfuseImport { application_id, .. } => (
                *application_id,
                ApplicationNonCrudConsoleOperation::LogsLangfuseImport,
            ),
        }
    }
    fn action(&self) -> &'static str {
        match self {
            Self::OtlpExport { .. } => "otlp_export",
            Self::OtlpImport { .. } => "otlp_import",
            Self::LangfuseExport { .. } => "langfuse_export",
            Self::LangfuseImport { .. } => "langfuse_import",
        }
    }
}
pub(crate) enum ExchangeOutput {
    Otlp(Value),
    Imported(ImportReceipt),
    Exported(LangfuseExportReceipt),
}
struct ExchangeAdapter {
    store: MainDurableStore,
    network: NetworkEgressHttpClientResolver,
}
impl ConsoleInterfacePort<ExchangeInput, ExchangeOutput> for ExchangeAdapter {
    fn execute<'a>(
        &'a self,
        principal: &'a UserPrincipal,
        input: ExchangeInput,
    ) -> ConsoleInterfaceFuture<'a, ExchangeOutput> {
        Box::pin(async move {
            let result = self.exchange(principal, input).await;
            result.map_err(|error| ConsoleInterfaceTargetError(error.into()))
        })
    }
}
impl ExchangeAdapter {
    async fn exchange(
        &self,
        principal: &UserPrincipal,
        input: ExchangeInput,
    ) -> anyhow::Result<ExchangeOutput> {
        let actor = principal.actor();
        let (application_id, operation) = input.identity();
        let store = self.store.for_actor(actor.clone());
        // Authorize the target before reading facts or using caller-supplied remote keys.
        let application = ApplicationService::new(store.clone())
            .load_application_for_non_crud_console_operation_for_actor(
                actor,
                application_id,
                operation,
            )
            .await?;
        if application.application_type != domain::ApplicationType::AgentLogs {
            return Err(ControlPlaneError::InvalidInput("agent_logs_application_type").into());
        }
        let service = AgentLogsService::new(store.clone());
        match input {
            ExchangeInput::OtlpExport { body, .. } => {
                let selector = AgentLogsExportSelector::from_ids(
                    body.record_ids,
                    body.log_conversation_ids,
                )
                .map_err(|_| ControlPlaneError::InvalidInput("agent_logs_export_selection"))?;
                let facts = store
                    .read_imported_agent_logs_selection(
                        application_id,
                        application.workspace_id,
                        &selector,
                    )
                    .await
                    .map_err(selection_error)?;
                Ok(ExchangeOutput::Otlp(serde_json::to_value(
                    protocol::batches_to_traces(&facts.batches)
                        .map_err(|_| ControlPlaneError::InvalidInput("agent_logs_otlp_export"))?,
                )?))
            }
            ExchangeInput::OtlpImport { body, .. } => {
                let request: protocol::ExportTraceServiceRequest =
                    serde_json::from_value(body.otlp)
                        .map_err(|_| ControlPlaneError::InvalidInput("agent_logs_otlp_payload"))?;
                let batches = protocol::traces_to_batches(&request)
                    .map_err(|_| ControlPlaneError::InvalidInput("agent_logs_otlp_payload"))?;
                let receipt = service
                    .import_console_batches(actor, application_id, operation, batches)
                    .await?;
                Ok(ExchangeOutput::Imported(receipt.into()))
            }
            ExchangeInput::LangfuseExport { body, .. } => {
                let base = remote::base_url(&body.base_url, &body.public_key, &body.secret_key)?;
                let selector = AgentLogsExportSelector::from_ids(
                    body.record_ids,
                    body.log_conversation_ids,
                )
                .map_err(|_| ControlPlaneError::InvalidInput("agent_logs_export_selection"))?;
                let facts = store
                    .read_imported_agent_logs_selection(
                        application_id,
                        application.workspace_id,
                        &selector,
                    )
                    .await
                    .map_err(selection_error)?;
                let request = protocol::batches_to_traces(&facts.batches)
                    .map_err(|_| ControlPlaneError::InvalidInput("agent_logs_otlp_export"))?;
                let trace_ids = request
                    .resource_spans
                    .iter()
                    .flat_map(|resource| &resource.scope_spans)
                    .flat_map(|scope| &scope.spans)
                    .map(|span| {
                        span.trace_id
                            .iter()
                            .map(|byte| format!("{byte:02x}"))
                            .collect::<String>()
                    })
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
                let exported_spans = request
                    .resource_spans
                    .iter()
                    .flat_map(|resource| &resource.scope_spans)
                    .map(|scope| scope.spans.len())
                    .sum();
                let exported_records = facts.record_ids.len();
                let scope = self
                    .network
                    .resolve_http_request(
                        actor.current_workspace_id,
                        domain::NetworkEgressConsumerSelector::AgentLogsExchange,
                    )
                    .await
                    .map_err(|_| {
                        ControlPlaneError::UpstreamUnavailable("agent_logs_network_egress")
                    })?;
                let result = remote::export(
                    scope.http_client(),
                    &base,
                    &body.public_key,
                    &body.secret_key,
                    &request,
                )
                .await;
                finish_network(scope, result).await?;
                Ok(ExchangeOutput::Exported(LangfuseExportReceipt {
                    exported_records,
                    exported_spans,
                    trace_ids,
                }))
            }
            ExchangeInput::LangfuseImport { body, .. } => {
                if application.sections.api.api_capability_status != "enabled" {
                    return Err(ControlPlaneError::PermissionDenied(
                        "agent_logs_ingestion_disabled",
                    )
                    .into());
                }
                let base = remote::base_url(&body.base_url, &body.public_key, &body.secret_key)?;
                let traces = remote::trace_ids(body.trace_ids)?;
                let scope = self
                    .network
                    .resolve_http_request(
                        actor.current_workspace_id,
                        domain::NetworkEgressConsumerSelector::AgentLogsExchange,
                    )
                    .await
                    .map_err(|_| {
                        ControlPlaneError::UpstreamUnavailable("agent_logs_network_egress")
                    })?;
                let result = remote::observations(
                    scope.http_client(),
                    &base,
                    &body.public_key,
                    &body.secret_key,
                    &traces,
                )
                .await;
                let observations = finish_network(scope, result).await?;
                // Gather every requested page and validate all mapped facts before
                // import can commit any source. The service rechecks actor/state.
                let batches = protocol::lf_observations_to_batches(&Value::Array(observations))
                    .map_err(|_| {
                        ControlPlaneError::InvalidInput("agent_logs_langfuse_observations")
                    })?;
                let receipt = service
                    .import_console_batches(actor, application_id, operation, batches)
                    .await?;
                Ok(ExchangeOutput::Imported(receipt.into()))
            }
        }
    }
}
fn selection_error(error: anyhow::Error) -> anyhow::Error {
    match error.to_string().as_str() {
        "agent_logs.read_selection_empty" => {
            ControlPlaneError::InvalidInput("agent_logs_record_ids").into()
        }
        "agent_logs.read_selection_invalid" => {
            ControlPlaneError::NotFound("agent_logs_record").into()
        }
        _ => ControlPlaneError::InvalidInput("agent_logs_source_facts").into(),
    }
}
async fn finish_network<T>(
    scope: NetworkEgressHttpRequestScope,
    result: anyhow::Result<T>,
) -> anyhow::Result<T> {
    scope.finish(result).await.map_err(|error| {
        if error.downcast_ref::<ControlPlaneError>().is_some() {
            error
        } else {
            ControlPlaneError::UpstreamUnavailable("agent_logs_network_egress").into()
        }
    })
}

#[cfg(test)]
mod _tests;

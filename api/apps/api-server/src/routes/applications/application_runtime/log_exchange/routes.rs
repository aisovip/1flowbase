use super::*;
use crate::routes::console_route_assembly::{console_post, ConsoleRouteAssembly};
use axum::handler::Handler;
use axum::{
    extract::{DefaultBodyLimit, Path, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    Json,
};

pub(crate) fn route_assembly() -> ConsoleRouteAssembly<Arc<ApiState>> {
    use access_control::ConsoleRouteOwnership::ConsoleOperation;
    ConsoleRouteAssembly::new()
        .route(
            "/applications/:id/logs/otlp/export",
            console_post(
                otlp_export.layer(DefaultBodyLimit::disable()),
                ConsoleOperation("applications.logs.otlp.export".to_string()),
            ),
        )
        .route(
            "/applications/:id/logs/otlp/import",
            console_post(
                otlp_import.layer(DefaultBodyLimit::disable()),
                ConsoleOperation("applications.logs.otlp.import".to_string()),
            ),
        )
        .route(
            "/applications/:id/logs/langfuse/export",
            console_post(
                langfuse_export.layer(DefaultBodyLimit::disable()),
                ConsoleOperation("applications.logs.langfuse.export".to_string()),
            ),
        )
        .route(
            "/applications/:id/logs/langfuse/import",
            console_post(
                langfuse_import.layer(DefaultBodyLimit::disable()),
                ConsoleOperation("applications.logs.langfuse.import".to_string()),
            ),
        )
}

#[utoipa::path(post, path = "/api/console/applications/{id}/logs/otlp/export",
    summary = "OTLP JSON file",
    description = "Download complete source facts for the selected imported records as a standard OTLP JSON ExportTraceServiceRequest. Reception may be disabled.",
    request_body = OtlpExportBody,
    params(("id" = Uuid, Path, description = "Agent logs application ID")),
    responses((status = 200, body = Value), (status = 400, body = crate::error_response::ErrorBody), (status = 401, body = crate::error_response::ErrorBody), (status = 403, body = crate::error_response::ErrorBody), (status = 404, body = crate::error_response::ErrorBody), (status = 409, body = crate::error_response::ErrorBody), (status = 502, body = crate::error_response::ErrorBody))
)]
pub(crate) async fn otlp_export(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(application_id): Path<Uuid>,
    Json(body): Json<OtlpExportBody>,
) -> Result<Response, ApiError> {
    let output: ExchangeOutput = console_interface::invoke(
        Arc::clone(&state),
        "http.console.applications.logs.otlp.export.v1",
        crate::extension_bus::ConsoleAuthenticationCredential::ProtocolWithCsrf { state, headers },
        ExchangeInput::OtlpExport {
            application_id,
            body,
        },
    )
    .await?;
    match output {
        ExchangeOutput::Otlp(value) => Ok((
            [(
                "content-disposition",
                "attachment; filename=agent-logs.otlp.json",
            )],
            Json(value),
        )
            .into_response()),
        _ => unreachable!("typed exchange operation output"),
    }
}

#[utoipa::path(post, path = "/api/console/applications/{id}/logs/otlp/import",
    summary = "OTLP JSON facts",
    description = "Import a standard OTLP JSON ExportTraceServiceRequest into an agent_logs application with reception enabled. Original source identities make retries idempotent; later source failures report committed counts.",
    request_body = OtlpImportBody,
    params(("id" = Uuid, Path, description = "Agent logs application ID")),
    responses((status = 200, body = ImportReceipt), (status = 400, body = crate::error_response::ErrorBody), (status = 401, body = crate::error_response::ErrorBody), (status = 403, body = crate::error_response::ErrorBody), (status = 404, body = crate::error_response::ErrorBody), (status = 409, body = crate::error_response::ErrorBody), (status = 502, body = crate::error_response::ErrorBody))
)]
pub(crate) async fn otlp_import(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(application_id): Path<Uuid>,
    Json(body): Json<OtlpImportBody>,
) -> Result<Response, ApiError> {
    let output: ExchangeOutput = console_interface::invoke(
        Arc::clone(&state),
        "http.console.applications.logs.otlp.import.v1",
        crate::extension_bus::ConsoleAuthenticationCredential::ProtocolWithCsrf { state, headers },
        ExchangeInput::OtlpImport {
            application_id,
            body,
        },
    )
    .await?;
    match output {
        ExchangeOutput::Imported(receipt) => Ok(Json(ApiSuccess::new(receipt)).into_response()),
        _ => unreachable!("typed exchange operation output"),
    }
}

#[utoipa::path(post, path = "/api/console/applications/{id}/logs/langfuse/export",
    summary = "Langfuse OTLP export",
    description = "Send selected imported source facts to the Langfuse OTLP endpoint using request-scoped Basic credentials and configured Network Egress. Return a receipt only for a complete successful acknowledgement.",
    request_body = LangfuseExportBody,
    params(("id" = Uuid, Path, description = "Agent logs application ID")),
    responses((status = 200, body = LangfuseExportReceipt), (status = 400, body = crate::error_response::ErrorBody), (status = 401, body = crate::error_response::ErrorBody), (status = 403, body = crate::error_response::ErrorBody), (status = 404, body = crate::error_response::ErrorBody), (status = 409, body = crate::error_response::ErrorBody), (status = 502, body = crate::error_response::ErrorBody))
)]
pub(crate) async fn langfuse_export(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(application_id): Path<Uuid>,
    Json(body): Json<LangfuseExportBody>,
) -> Result<Response, ApiError> {
    let output: ExchangeOutput = console_interface::invoke(
        Arc::clone(&state),
        "http.console.applications.logs.langfuse.export.v1",
        crate::extension_bus::ConsoleAuthenticationCredential::ProtocolWithCsrf { state, headers },
        ExchangeInput::LangfuseExport {
            application_id,
            body,
        },
    )
    .await?;
    match output {
        ExchangeOutput::Exported(receipt) => Ok(Json(ApiSuccess::new(receipt)).into_response()),
        _ => unreachable!("typed exchange operation output"),
    }
}

#[utoipa::path(post, path = "/api/console/applications/{id}/logs/langfuse/import",
    summary = "Langfuse observations",
    description = "Gather every page of every selected Langfuse trace, preserving expanded original metadata, before importing source batches. Require reception enabled; credentials remain request-scoped.",
    request_body = LangfuseImportBody,
    params(("id" = Uuid, Path, description = "Agent logs application ID")),
    responses((status = 200, body = ImportReceipt), (status = 400, body = crate::error_response::ErrorBody), (status = 401, body = crate::error_response::ErrorBody), (status = 403, body = crate::error_response::ErrorBody), (status = 404, body = crate::error_response::ErrorBody), (status = 409, body = crate::error_response::ErrorBody), (status = 502, body = crate::error_response::ErrorBody))
)]
pub(crate) async fn langfuse_import(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(application_id): Path<Uuid>,
    Json(body): Json<LangfuseImportBody>,
) -> Result<Response, ApiError> {
    let output: ExchangeOutput = console_interface::invoke(
        Arc::clone(&state),
        "http.console.applications.logs.langfuse.import.v1",
        crate::extension_bus::ConsoleAuthenticationCredential::ProtocolWithCsrf { state, headers },
        ExchangeInput::LangfuseImport {
            application_id,
            body,
        },
    )
    .await?;
    match output {
        ExchangeOutput::Imported(receipt) => Ok(Json(ApiSuccess::new(receipt)).into_response()),
        _ => unreachable!("typed exchange operation output"),
    }
}

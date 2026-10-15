//! Request-scoped Basic credentials and complete, bounded-by-source pagination.
//! No URL, credential, remote error body or raw fact is used as an error message.
use super::*;
use anyhow::{ensure, Result};
use reqwest::{Client, StatusCode};
use std::collections::BTreeSet;
use url::Url;

const FIELDS: &str = "basic,time,io,metadata,model,usage,trace_context";
const EXPAND_METADATA: &str = "1flowbase_agent_log";

pub(super) fn base_url(value: &str, public: &str, secret: &str) -> Result<Url> {
    ensure!(
        !public.trim().is_empty() && !secret.trim().is_empty(),
        ControlPlaneError::InvalidInput("agent_logs_langfuse_credentials")
    );
    let mut url = Url::parse(value)
        .map_err(|_| ControlPlaneError::InvalidInput("agent_logs_langfuse_base_url"))?;
    ensure!(
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        ControlPlaneError::InvalidInput("agent_logs_langfuse_base_url")
    );
    if !url.path().ends_with('/') {
        url.set_path(&format!("{}/", url.path()));
    }
    Ok(url)
}
pub(super) fn trace_ids(ids: Vec<String>) -> Result<Vec<String>> {
    ensure!(
        !ids.is_empty() && ids.iter().all(|id| !id.trim().is_empty()),
        ControlPlaneError::InvalidInput("agent_logs_trace_ids")
    );
    let mut seen = BTreeSet::new();
    Ok(ids
        .into_iter()
        .filter(|id| seen.insert(id.clone()))
        .collect())
}
pub(super) async fn export(
    client: &Client,
    base: &Url,
    public: &str,
    secret: &str,
    request: &protocol::ExportTraceServiceRequest,
) -> Result<()> {
    use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceResponse;
    use prost::Message;
    let url = base
        .join("api/public/otel/v1/traces")
        .map_err(|_| ControlPlaneError::InvalidInput("agent_logs_langfuse_base_url"))?;
    let body = protocol::encode_json(request)
        .map_err(|_| ControlPlaneError::InvalidInput("agent_logs_otlp_export"))?;
    let response = client
        .post(url)
        .basic_auth(public, Some(secret))
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|_| ControlPlaneError::UpstreamUnavailable("agent_logs_langfuse_network"))?;
    if !response.status().is_success() {
        return Err(http_error(response.status()).into());
    }
    let mime = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let bytes = response
        .bytes()
        .await
        .map_err(|_| ControlPlaneError::UpstreamUnavailable("agent_logs_langfuse_network"))?;
    let ack: ExportTraceServiceResponse = match mime.as_str() {
        "application/json" => serde_json::from_slice(&bytes)
            .map_err(|_| ControlPlaneError::UpstreamUnavailable("agent_logs_langfuse_ack"))?,
        "application/x-protobuf" => ExportTraceServiceResponse::decode(bytes.as_ref())
            .map_err(|_| ControlPlaneError::UpstreamUnavailable("agent_logs_langfuse_ack"))?,
        _ => return Err(ControlPlaneError::UpstreamUnavailable("agent_logs_langfuse_ack").into()),
    };
    if let Some(partial) = ack.partial_success {
        ensure!(
            partial.rejected_spans >= 0,
            ControlPlaneError::UpstreamUnavailable("agent_logs_langfuse_ack")
        );
        // A warning is still a partial acknowledgement and cannot become the
        // clean full receipt promised by this Console operation's frozen DTO.
        ensure!(
            partial.rejected_spans == 0 && partial.error_message.is_empty(),
            ControlPlaneError::UpstreamUnavailable("agent_logs_langfuse_partial_export")
        );
    }
    Ok(())
}

pub(super) async fn observations(
    client: &Client,
    base: &Url,
    public: &str,
    secret: &str,
    traces: &[String],
) -> Result<Vec<Value>> {
    let mut result = Vec::new();
    let mut legacy = false;
    for trace in traces {
        let loaded = if legacy {
            legacy_observations(client, base, public, secret, trace).await?
        } else {
            match v2_observations(client, base, public, secret, trace).await? {
                Some(data) => data,
                None => {
                    legacy = true;
                    legacy_observations(client, base, public, secret, trace).await?
                }
            }
        };
        ensure!(
            !loaded.is_empty(),
            ControlPlaneError::NotFound("agent_logs_langfuse_trace")
        );
        result.extend(loaded);
    }
    Ok(result)
}
async fn v2_observations(
    client: &Client,
    base: &Url,
    public: &str,
    secret: &str,
    trace: &str,
) -> Result<Option<Vec<Value>>> {
    let endpoint = base
        .join("api/public/v2/observations")
        .map_err(|_| ControlPlaneError::InvalidInput("agent_logs_langfuse_base_url"))?;
    let mut data = Vec::new();
    let mut cursor: Option<String> = None;
    let mut seen = BTreeSet::new();
    loop {
        let mut request = client
            .get(endpoint.clone())
            .basic_auth(public, Some(secret))
            .query(&[
                ("traceId", trace),
                ("fields", FIELDS),
                ("expandMetadata", EXPAND_METADATA),
            ]);
        if let Some(cursor) = &cursor {
            request = request.query(&[("cursor", cursor)]);
        }
        let response = request
            .send()
            .await
            .map_err(|_| ControlPlaneError::UpstreamUnavailable("agent_logs_langfuse_network"))?;
        let status = response.status();
        let mime = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_owned();
        let body = response
            .bytes()
            .await
            .map_err(|_| ControlPlaneError::UpstreamUnavailable("agent_logs_langfuse_network"))?;
        if !status.is_success() {
            if status == StatusCode::NOT_FOUND && cursor.is_none() && capability_404(&mime, &body) {
                return Ok(None);
            }
            return Err(http_error(status).into());
        }
        let page: Value = serde_json::from_slice(&body)
            .map_err(|_| ControlPlaneError::UpstreamUnavailable("agent_logs_langfuse_response"))?;
        let items = validated_page(&page, trace)?;
        let next = next_cursor(&page)?;
        ensure!(
            !items.is_empty() || next.is_none(),
            ControlPlaneError::UpstreamUnavailable("agent_logs_langfuse_cursor")
        );
        data.extend(items.iter().cloned());
        let Some(next) = next else {
            return Ok(Some(data));
        };
        ensure!(
            seen.insert(next.clone()),
            ControlPlaneError::UpstreamUnavailable("agent_logs_langfuse_cursor")
        );
        cursor = Some(next);
    }
}
fn next_cursor(page: &Value) -> Result<Option<String>> {
    let meta = page.get("meta").and_then(Value::as_object).ok_or(
        ControlPlaneError::UpstreamUnavailable("agent_logs_langfuse_cursor"),
    )?;
    let read = |key: &str| -> Result<Option<String>> {
        match meta.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(value)) if !value.is_empty() => Ok(Some(value.clone())),
            // An empty opaque cursor is not a valid continuation token.
            _ => Err(ControlPlaneError::UpstreamUnavailable("agent_logs_langfuse_cursor").into()),
        }
    };
    let cursor = read("cursor")?;
    let next = read("nextCursor")?;
    ensure!(
        cursor.is_none() || next.is_none() || cursor == next,
        ControlPlaneError::UpstreamUnavailable("agent_logs_langfuse_cursor")
    );
    ensure!(
        meta.is_empty() || meta.contains_key("cursor") || meta.contains_key("nextCursor"),
        ControlPlaneError::UpstreamUnavailable("agent_logs_langfuse_cursor")
    );
    Ok(cursor.or(next))
}
async fn legacy_observations(
    client: &Client,
    base: &Url,
    public: &str,
    secret: &str,
    trace: &str,
) -> Result<Vec<Value>> {
    let endpoint = base
        .join("api/public/observations")
        .map_err(|_| ControlPlaneError::InvalidInput("agent_logs_langfuse_base_url"))?;
    let mut data = Vec::new();
    let mut page_number = 1_u64;
    loop {
        let response = client
            .get(endpoint.clone())
            .basic_auth(public, Some(secret))
            .query(&[
                ("traceId", trace.to_owned()),
                ("page", page_number.to_string()),
            ])
            .send()
            .await
            .map_err(|_| ControlPlaneError::UpstreamUnavailable("agent_logs_langfuse_network"))?;
        if !response.status().is_success() {
            return Err(http_error(response.status()).into());
        }
        let page: Value = response
            .json()
            .await
            .map_err(|_| ControlPlaneError::UpstreamUnavailable("agent_logs_langfuse_response"))?;
        let items = validated_page(&page, trace)?;
        let meta = page.get("meta").and_then(Value::as_object).ok_or(
            ControlPlaneError::UpstreamUnavailable("agent_logs_langfuse_cursor"),
        )?;
        let total_pages = meta.get("totalPages").and_then(Value::as_u64).ok_or(
            ControlPlaneError::UpstreamUnavailable("agent_logs_langfuse_cursor"),
        )?;
        ensure!(
            meta.get("page").and_then(Value::as_u64) == Some(page_number),
            ControlPlaneError::UpstreamUnavailable("agent_logs_langfuse_cursor")
        );
        ensure!(
            total_pages >= page_number
                || (page_number == 1 && total_pages == 0 && items.is_empty()),
            ControlPlaneError::UpstreamUnavailable("agent_logs_langfuse_cursor")
        );
        ensure!(
            !items.is_empty() || page_number >= total_pages,
            ControlPlaneError::UpstreamUnavailable("agent_logs_langfuse_cursor")
        );
        data.extend(items.iter().cloned());
        if page_number >= total_pages {
            return Ok(data);
        }
        page_number = page_number
            .checked_add(1)
            .ok_or(ControlPlaneError::UpstreamUnavailable(
                "agent_logs_langfuse_cursor",
            ))?;
    }
}
fn validated_page<'a>(page: &'a Value, trace: &str) -> Result<&'a Vec<Value>> {
    let items = page.get("data").and_then(Value::as_array).ok_or(
        ControlPlaneError::UpstreamUnavailable("agent_logs_langfuse_response"),
    )?;
    ensure!(
        items
            .iter()
            .all(|item| item.get("traceId").and_then(Value::as_str) == Some(trace)),
        ControlPlaneError::UpstreamUnavailable("agent_logs_langfuse_trace_mismatch")
    );
    Ok(items)
}
fn capability_404(mime: &str, body: &[u8]) -> bool {
    let message = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_default()
        .to_ascii_lowercase();
    message.contains("observations v2 api is only available")
        || ((message.contains("endpoint") || message.contains("route"))
            && (message.contains("not found")
                || message.contains("unsupported")
                || message.contains("unknown")))
        || (mime.starts_with("text/html")
            && String::from_utf8_lossy(body).contains("This page could not be found"))
}
fn http_error(status: StatusCode) -> ControlPlaneError {
    ControlPlaneError::UpstreamUnavailable(match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => "agent_logs_langfuse_authentication",
        StatusCode::TOO_MANY_REQUESTS => "agent_logs_langfuse_rate_limited",
        StatusCode::NOT_FOUND => "agent_logs_langfuse_not_found",
        _ => "agent_logs_langfuse_http_error",
    })
}

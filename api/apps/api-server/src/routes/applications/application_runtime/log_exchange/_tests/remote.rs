use super::*;
use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode, Uri},
    response::IntoResponse,
    routing::any,
    Router,
};
use std::{collections::VecDeque, sync::Mutex};

struct Seen {
    uri: String,
    authorization: String,
    body: Vec<u8>,
}
struct Mock {
    replies: Mutex<VecDeque<(StatusCode, &'static str, Vec<u8>)>>,
    seen: Mutex<Vec<Seen>>,
}
struct Server {
    task: tokio::task::JoinHandle<()>,
    base: url::Url,
    mock: Arc<Mock>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn server(replies: Vec<(StatusCode, Value)>) -> Server {
    server_raw(
        replies
            .into_iter()
            .map(|(status, body)| (status, "application/json", body.to_string().into_bytes()))
            .collect(),
    )
    .await
}
async fn server_raw(replies: Vec<(StatusCode, &'static str, Vec<u8>)>) -> Server {
    let mock = Arc::new(Mock {
        replies: Mutex::new(replies.into()),
        seen: Mutex::new(vec![]),
    });
    async fn respond(
        State(mock): State<Arc<Mock>>,
        uri: Uri,
        headers: HeaderMap,
        body: Bytes,
    ) -> impl IntoResponse {
        mock.seen.lock().unwrap().push(Seen {
            uri: uri.to_string(),
            authorization: headers
                .get("authorization")
                .unwrap()
                .to_str()
                .unwrap()
                .into(),
            body: body.to_vec(),
        });
        let (status, mime, body) = mock
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected remote call");
        (status, [("content-type", mime)], body)
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = url::Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
    let app = Router::new()
        .fallback(any(respond))
        .with_state(mock.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    Server { task, base, mock }
}
async fn scope() -> NetworkEgressHttpRequestScope {
    NetworkEgressHttpClientResolver::direct_for_tests()
        .resolve_http_request(
            Uuid::now_v7(),
            domain::NetworkEgressConsumerSelector::AgentLogsExchange,
        )
        .await
        .unwrap()
}
fn page(trace: &str, meta: Value) -> Value {
    json!({"data":[{"id":"span","traceId":trace,"type":"SPAN","startTime":"2026-10-07T00:00:00Z"}],"meta":meta})
}

#[tokio::test]
async fn langfuse_exhausts_cursor_pages_and_traces_with_full_metadata() {
    let server = server(vec![
        (StatusCode::OK, page("a", json!({"cursor":"opaque"}))),
        (StatusCode::OK, page("a", json!({}))),
        (StatusCode::OK, page("b", json!({"nextCursor":"variant"}))),
        (StatusCode::OK, page("b", json!({"cursor":null}))),
    ])
    .await;
    let scope = scope().await;
    let result = super::super::remote::observations(
        scope.http_client(),
        &server.base,
        "pk",
        "sk",
        &["a".into(), "b".into()],
    )
    .await;
    assert_eq!(scope.finish(result).await.unwrap().len(), 4);
    let seen = server.mock.seen.lock().unwrap();
    assert_eq!(seen.len(), 4);
    for seen in seen.iter() {
        assert!(seen.uri.starts_with("/api/public/v2/observations?"));
        assert!(seen.uri.contains("expandMetadata=1flowbase_agent_log"));
        assert!(seen
            .uri
            .contains("fields=basic%2Ctime%2Cio%2Cmetadata%2Cmodel%2Cusage%2Ctrace_context"));
        assert_eq!(seen.authorization, "Basic cGs6c2s=");
    }
    assert!(seen[1].uri.contains("cursor=opaque"));
    assert!(seen[3].uri.contains("cursor=variant"));
}

#[tokio::test]
async fn langfuse_partial_warning_and_malformed_ack_refuse_full_receipt() {
    for ack in [
        json!({"partialSuccess":{"rejectedSpans":"1","errorMessage":"private upstream detail"}}),
        json!({"partialSuccess":{"rejectedSpans":"0","errorMessage":"warning"}}),
        json!({"partialSuccess":{"rejectedSpans":"-1"}}),
        json!({"partialSuccess":"bad"}),
    ] {
        let server = server(vec![(StatusCode::OK, ack)]).await;
        let scope = scope().await;
        let result = super::super::remote::export(
            scope.http_client(),
            &server.base,
            "pk",
            "sk",
            &protocol::ExportTraceServiceRequest::default(),
        )
        .await;
        let error = scope.finish(result).await.unwrap_err();
        assert!(!error.to_string().contains("private upstream detail"));
    }
    let server = server(vec![(StatusCode::OK, json!({}))]).await;
    let scope = scope().await;
    let result = super::super::remote::export(
        scope.http_client(),
        &server.base,
        "pk",
        "sk",
        &protocol::ExportTraceServiceRequest::default(),
    )
    .await;
    scope.finish(result).await.unwrap();
    let seen = server.mock.seen.lock().unwrap();
    assert_eq!(seen[0].uri, "/api/public/otel/v1/traces");
    assert_eq!(seen[0].authorization, "Basic cGs6c2s=");
    let body: Value = serde_json::from_slice(&seen[0].body).unwrap();
    assert!(body
        .as_object()
        .unwrap()
        .keys()
        .all(|key| key == "resourceSpans"));
}

#[tokio::test]
async fn langfuse_only_capability_404_falls_back_to_paginated_legacy() {
    let capability_server = server(vec![
        (
            StatusCode::NOT_FOUND,
            json!({"message":"Observations v2 API is only available in v4 write mode"}),
        ),
        (StatusCode::OK, page("a", json!({"page":1,"totalPages":2}))),
        (StatusCode::OK, page("a", json!({"page":2,"totalPages":2}))),
    ])
    .await;
    let capability_scope = scope().await;
    let result = super::super::remote::observations(
        capability_scope.http_client(),
        &capability_server.base,
        "pk",
        "sk",
        &["a".into()],
    )
    .await;
    assert_eq!(capability_scope.finish(result).await.unwrap().len(), 2);
    assert!(capability_server.mock.seen.lock().unwrap()[2]
        .uri
        .contains("/api/public/observations?traceId=a&page=2"));
    for status in [
        StatusCode::NOT_FOUND,
        StatusCode::UNAUTHORIZED,
        StatusCode::FORBIDDEN,
        StatusCode::TOO_MANY_REQUESTS,
        StatusCode::INTERNAL_SERVER_ERROR,
    ] {
        let server = server(vec![(status, json!({"message":"secret remote error"}))]).await;
        let scope = scope().await;
        let result = super::super::remote::observations(
            scope.http_client(),
            &server.base,
            "pk",
            "sk",
            &["a".into()],
        )
        .await;
        assert!(scope.finish(result).await.is_err());
        assert_eq!(server.mock.seen.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn langfuse_cursor_conflicts_cycles_and_trace_mismatch_abort_gather() {
    let cases = vec![
        vec![page("a", json!({"cursor":"x","nextCursor":"y"}))],
        vec![
            page("a", json!({"cursor":"x"})),
            page("a", json!({"cursor":"x"})),
        ],
        vec![json!({"data":[{"id":"span"}],"meta":{}})],
        vec![page("wrong", json!({}))],
        vec![page("a", json!({})), page("wrong", json!({}))],
    ];
    for (index, pages) in cases.into_iter().enumerate() {
        let server = server(
            pages
                .into_iter()
                .map(|page| (StatusCode::OK, page))
                .collect(),
        )
        .await;
        let scope = scope().await;
        let traces = if index == 4 {
            vec!["a".into(), "b".into()]
        } else {
            vec!["a".into()]
        };
        let result = super::super::remote::observations(
            scope.http_client(),
            &server.base,
            "pk",
            "sk",
            &traces,
        )
        .await;
        assert!(scope.finish(result).await.is_err());
    }
}

#[test]
fn langfuse_destination_and_trace_selection_are_unambiguous() {
    for url in [
        "ftp://example.com",
        "https://user:secret@example.com",
        "https://example.com?key=secret",
        "https://example.com#secret",
    ] {
        assert!(super::super::remote::base_url(url, "pk", "sk").is_err());
    }
    assert!(super::super::remote::base_url("https://example.com", "", "sk").is_err());
    assert!(super::super::remote::trace_ids(vec![]).is_err());
    assert_eq!(
        super::super::remote::trace_ids(vec!["a".into(), "a".into(), "b".into()]).unwrap(),
        vec!["a", "b"]
    );
}

#[tokio::test]
async fn langfuse_protobuf_ack_and_transport_errors_use_safe_protocol_outcomes() {
    use opentelemetry_proto::tonic::collector::trace::v1::{
        ExportTracePartialSuccess, ExportTraceServiceResponse,
    };
    use prost::Message;
    for (ack, success) in [
        (ExportTraceServiceResponse::default(), true),
        (
            ExportTraceServiceResponse {
                partial_success: Some(ExportTracePartialSuccess {
                    rejected_spans: 1,
                    error_message: "private remote message".into(),
                }),
            },
            false,
        ),
    ] {
        let server = server_raw(vec![(
            StatusCode::OK,
            "application/x-protobuf",
            ack.encode_to_vec(),
        )])
        .await;
        let scope = scope().await;
        let result = super::super::remote::export(
            scope.http_client(),
            &server.base,
            "pk",
            "sk",
            &protocol::ExportTraceServiceRequest::default(),
        )
        .await;
        assert_eq!(scope.finish(result).await.is_ok(), success);
    }
    for (mime, body) in [
        ("application/json", b"invalid".to_vec()),
        ("application/x-protobuf", vec![255]),
        ("text/plain", b"ok".to_vec()),
    ] {
        let server = server_raw(vec![(StatusCode::OK, mime, body)]).await;
        let scope = scope().await;
        let result = super::super::remote::export(
            scope.http_client(),
            &server.base,
            "pk",
            "sk",
            &protocol::ExportTraceServiceRequest::default(),
        )
        .await;
        assert!(scope.finish(result).await.is_err());
    }
    let scope = scope().await;
    let result = super::super::remote::observations(
        scope.http_client(),
        &url::Url::parse("http://127.0.0.1:0/").unwrap(),
        "pk",
        "sk",
        &["a".into()],
    )
    .await;
    let error = scope.finish(result).await.unwrap_err();
    assert_eq!(
        error
            .downcast_ref::<ControlPlaneError>()
            .unwrap()
            .to_string(),
        "upstream unavailable: agent_logs_langfuse_network"
    );
}

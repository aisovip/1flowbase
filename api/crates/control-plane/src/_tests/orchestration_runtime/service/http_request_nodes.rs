use super::*;
use std::net::SocketAddr;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

async fn spawn_binary_response_server() -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buffer = vec![0_u8; 1024];
                let _ = stream.read(&mut buffer).await;
                let body = [0_u8, 1, 2, 3];
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
                stream.write_all(&body).await.unwrap();
            });
        }
    });

    (format!("http://{addr}/download"), handle)
}

async fn spawn_text_response_server(
    content_type: &'static str,
    body: &'static str,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buffer = vec![0_u8; 1024];
                let _ = stream.read(&mut buffer).await;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    content_type,
                    body
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            });
        }
    });

    (format!("http://{addr}/ok"), handle)
}

fn http_request_flow_document(flow_id: Uuid, url: String) -> Value {
    json!({
        "schemaVersion": "1flowbase.flow/v2",
        "meta": {
            "flowId": flow_id.to_string(),
            "name": "HTTP Request Agent",
            "description": "",
            "tags": []
        },
        "graph": {
            "nodes": [
                {
                    "id": "node-start",
                    "type": "start",
                    "alias": "Start",
                    "description": "",
                    "containerId": null,
                    "position": { "x": 0, "y": 0 },
                    "configVersion": 1,
                    "config": {},
                    "bindings": {},
                    "outputs": []
                },
                {
                    "id": "node-http",
                    "type": "http_request",
                    "alias": "HTTP Request",
                    "description": "",
                    "containerId": null,
                    "position": { "x": 240, "y": 0 },
                    "configVersion": 1,
                    "config": {
                        "method": "GET",
                        "url": url,
                        "body_type": "none",
                        "timeout_ms": 30000,
                        "max_response_bytes": 1048576,
                        "verify_ssl": true
                    },
                    "bindings": {
                        "params": { "kind": "named_bindings", "value": [] },
                        "headers": { "kind": "named_bindings", "value": [] },
                        "body": { "kind": "templated_text", "value": "" },
                        "urlencoded": { "kind": "named_bindings", "value": [] },
                        "form_data": { "kind": "named_bindings", "value": [] }
                    },
                    "outputs": [
                        { "key": "body", "title": "HTTP 响应正文", "valueType": "string" },
                        { "key": "status_code", "title": "响应状态码", "valueType": "number" },
                        { "key": "headers", "title": "响应头列表 JSON", "valueType": "object" },
                        { "key": "files", "title": "HTTP 响应文件", "valueType": "Array[File]" }
                    ]
                }
            ],
            "edges": [
                {
                    "id": "edge-start-http",
                    "source": "node-start",
                    "target": "node-http",
                    "sourceHandle": null,
                    "targetHandle": null,
                    "containerId": null,
                    "points": []
                }
            ]
        },
        "editor": {
            "viewport": { "x": 0, "y": 0, "zoom": 1 },
            "annotations": [],
            "activeContainerPath": []
        }
    })
}

fn http_request_flow_document_with_unrelated_invalid_llm(flow_id: Uuid, url: String) -> Value {
    let mut document = http_request_flow_document(flow_id, url);
    let nodes = document["graph"]["nodes"].as_array_mut().unwrap();
    nodes.push(json!({
        "id": "node-llm",
        "type": "llm",
        "alias": "LLM",
        "description": "",
        "containerId": null,
        "position": { "x": 480, "y": 0 },
        "configVersion": 1,
        "config": {
            "model_provider": {
                "provider_code": "fixture_provider",
                "model_id": "not-enabled-model"
            },
            "temperature": 0.2
        },
        "bindings": {
            "prompt_messages": {
                "kind": "prompt_messages",
                "value": [
                    {
                        "id": "user-1",
                        "role": "user",
                        "content": {
                            "kind": "templated_text",
                            "value": "{{node-start.query}}"
                        }
                    }
                ]
            }
        },
        "outputs": [{ "key": "text", "title": "模型输出", "valueType": "string" }]
    }));
    document
}

fn http_request_flow_document_with_store_response_as_file(flow_id: Uuid, url: String) -> Value {
    let mut document = http_request_flow_document(flow_id, url);
    document["graph"]["nodes"][1]["config"]["store_response_as_file"] = json!(true);
    document
}

fn http_request_flow_document_with_max_response_bytes(
    flow_id: Uuid,
    url: String,
    max_response_bytes: u64,
) -> Value {
    let mut document = http_request_flow_document(flow_id, url);
    document["graph"]["nodes"][1]["config"]["max_response_bytes"] = json!(max_response_bytes);
    document
}

#[tokio::test]
async fn http_request_node_preview_ignores_unrelated_llm_model_compile_issue() {
    let (url, server) = spawn_text_response_server("text/plain", "ok").await;
    let service = OrchestrationRuntimeService::for_tests();
    let seeded = service.seed_application_with_flow("HTTP Agent").await;

    let outcome = service
        .start_node_debug_preview(StartNodeDebugPreviewCommand {
            actor_user_id: seeded.actor_user_id,
            application_id: seeded.application_id,
            node_id: "node-http".to_string(),
            input_payload: json!({}),
            document_snapshot: Some(http_request_flow_document_with_unrelated_invalid_llm(
                seeded.flow_id,
                url,
            )),
            debug_session_id: None,
        })
        .await
        .expect("HTTP node preview should ignore unrelated LLM compile issues");

    server.abort();

    assert_eq!(outcome.flow_run.status, domain::FlowRunStatus::Succeeded);
    assert_eq!(outcome.node_run.status, domain::NodeRunStatus::Succeeded);
    assert_eq!(outcome.node_run.output_payload["status_code"], json!(200));
    assert_eq!(outcome.node_run.output_payload["body"], json!("ok"));
}

#[tokio::test]
async fn http_request_javascript_response_stays_inline_with_file_storage() {
    let body = r#"jQuery1123({"data":{"total":5}});"#;
    let (url, server) =
        spawn_text_response_server("application/javascript; charset=UTF-8", body).await;
    let service = OrchestrationRuntimeService::for_tests_with_file_storage();
    let seeded = service.seed_application_with_flow("HTTP Agent").await;
    let started = service
        .start_flow_debug_run(StartFlowDebugRunCommand {
            actor_user_id: seeded.actor_user_id,
            application_id: seeded.application_id,
            input_payload: json!({}),
            document_snapshot: Some(http_request_flow_document(seeded.flow_id, url)),
            debug_session_id: None,
        })
        .await
        .unwrap();

    let completed = service
        .continue_flow_debug_run(ContinueFlowDebugRunCommand {
            application_id: seeded.application_id,
            flow_run_id: started.flow_run.id,
            workspace_id: Uuid::nil(),
        })
        .await
        .unwrap();

    let http_node = node_run(&completed, "node-http");

    assert_eq!(completed.flow_run.status, domain::FlowRunStatus::Succeeded);
    assert_eq!(http_node.status, domain::NodeRunStatus::Succeeded);
    assert_eq!(http_node.output_payload["body"], json!(body));
    assert_eq!(http_node.output_payload["files"], json!([]));

    server.abort();
}

#[tokio::test]
async fn http_request_store_response_as_file_keeps_text_response_inline_with_file_storage() {
    let body = r#"jQuery1123({"data":{"total":5}});"#;
    let (url, server) =
        spawn_text_response_server("application/javascript; charset=UTF-8", body).await;
    let service = OrchestrationRuntimeService::for_tests_with_file_storage();
    let seeded = service.seed_application_with_flow("HTTP Agent").await;
    let started = service
        .start_flow_debug_run(StartFlowDebugRunCommand {
            actor_user_id: seeded.actor_user_id,
            application_id: seeded.application_id,
            input_payload: json!({}),
            document_snapshot: Some(http_request_flow_document_with_store_response_as_file(
                seeded.flow_id,
                url,
            )),
            debug_session_id: None,
        })
        .await
        .unwrap();

    let completed = service
        .continue_flow_debug_run(ContinueFlowDebugRunCommand {
            application_id: seeded.application_id,
            flow_run_id: started.flow_run.id,
            workspace_id: Uuid::nil(),
        })
        .await
        .unwrap();

    let http_node = node_run(&completed, "node-http");

    assert_eq!(completed.flow_run.status, domain::FlowRunStatus::Succeeded);
    assert_eq!(http_node.status, domain::NodeRunStatus::Succeeded);
    assert_eq!(http_node.output_payload["body"], json!(body));
    assert_eq!(http_node.output_payload["files"], json!([]));

    server.abort();
}

#[tokio::test]
async fn http_request_binary_response_stays_inline_when_file_storage_is_disabled() {
    let (url, server) = spawn_binary_response_server().await;
    let service = OrchestrationRuntimeService::for_tests_with_file_storage();
    let seeded = service.seed_application_with_flow("HTTP Agent").await;
    let started = service
        .start_flow_debug_run(StartFlowDebugRunCommand {
            actor_user_id: seeded.actor_user_id,
            application_id: seeded.application_id,
            input_payload: json!({}),
            document_snapshot: Some(http_request_flow_document(seeded.flow_id, url)),
            debug_session_id: None,
        })
        .await
        .unwrap();

    let completed = service
        .continue_flow_debug_run(ContinueFlowDebugRunCommand {
            application_id: seeded.application_id,
            flow_run_id: started.flow_run.id,
            workspace_id: Uuid::nil(),
        })
        .await
        .unwrap();

    let http_node = node_run(&completed, "node-http");

    assert_eq!(completed.flow_run.status, domain::FlowRunStatus::Succeeded);
    assert_eq!(http_node.status, domain::NodeRunStatus::Succeeded);
    assert_eq!(
        http_node.output_payload["body"],
        json!(String::from_utf8_lossy(&[0, 1, 2, 3]).to_string())
    );
    assert_eq!(http_node.output_payload["files"], json!([]));

    server.abort();
}

#[tokio::test]
async fn http_request_binary_response_persists_as_file_record_when_enabled() {
    let (url, server) = spawn_binary_response_server().await;
    let service = OrchestrationRuntimeService::for_tests_with_file_storage();
    let seeded = service.seed_application_with_flow("HTTP Agent").await;
    let started = service
        .start_flow_debug_run(StartFlowDebugRunCommand {
            actor_user_id: seeded.actor_user_id,
            application_id: seeded.application_id,
            input_payload: json!({}),
            document_snapshot: Some(http_request_flow_document_with_store_response_as_file(
                seeded.flow_id,
                url,
            )),
            debug_session_id: None,
        })
        .await
        .unwrap();

    let completed = service
        .continue_flow_debug_run(ContinueFlowDebugRunCommand {
            application_id: seeded.application_id,
            flow_run_id: started.flow_run.id,
            workspace_id: Uuid::nil(),
        })
        .await
        .unwrap();

    let http_node = node_run(&completed, "node-http");
    let file = &http_node.output_payload["files"][0];

    assert_eq!(completed.flow_run.status, domain::FlowRunStatus::Succeeded);
    assert_eq!(http_node.status, domain::NodeRunStatus::Succeeded);
    assert!(file["id"].as_str().is_some());
    assert_eq!(file["filename"], json!("response.bin"));
    assert_eq!(file["mimetype"], json!("application/octet-stream"));
    assert_eq!(file["storage_id"], service.default_file_storage_id_json());
    assert!(file["path"].as_str().unwrap().contains("/orders/"));
    let record_id = Uuid::parse_str(file["id"].as_str().unwrap()).unwrap();
    let file_table_id = service.default_file_table_id().await;
    assert_eq!(
        file["url"],
        json!(format!(
            "/api/console/files/{file_table_id}/records/{record_id}/content"
        ))
    );
    assert_eq!(file["size"], json!(4));
    assert_eq!(file["meta"]["source"], json!("http_request_response"));
    assert_eq!(file["meta"]["node_id"], json!("node-http"));
    assert_ne!(file["storage_id"], json!("runtime-inline"));
    assert_ne!(file["meta"]["persisted"], json!(false));

    server.abort();
}

#[tokio::test]
async fn http_request_truncates_large_response_before_persisting_node_output() {
    let (url, server) = spawn_text_response_server("text/plain", "abcdef").await;
    let service = OrchestrationRuntimeService::for_tests_with_file_storage();
    let seeded = service.seed_application_with_flow("HTTP Agent").await;
    let started = service
        .start_flow_debug_run(StartFlowDebugRunCommand {
            actor_user_id: seeded.actor_user_id,
            application_id: seeded.application_id,
            input_payload: json!({}),
            document_snapshot: Some(http_request_flow_document_with_max_response_bytes(
                seeded.flow_id,
                url,
                4,
            )),
            debug_session_id: None,
        })
        .await
        .unwrap();

    let completed = service
        .continue_flow_debug_run(ContinueFlowDebugRunCommand {
            application_id: seeded.application_id,
            flow_run_id: started.flow_run.id,
            workspace_id: Uuid::nil(),
        })
        .await
        .unwrap();

    let http_node = node_run(&completed, "node-http");

    assert_eq!(completed.flow_run.status, domain::FlowRunStatus::Succeeded);
    assert_eq!(http_node.status, domain::NodeRunStatus::Succeeded);
    assert_eq!(http_node.output_payload["body"], json!("abcd"));
    assert_eq!(http_node.output_payload["files"], json!([]));
    assert_eq!(http_node.metrics_payload["body_truncated"], json!(true));
    assert_eq!(
        http_node.metrics_payload["response_bytes_observed"],
        json!(6)
    );
    assert_eq!(http_node.metrics_payload["stored_body_bytes"], json!(4));

    server.abort();
}

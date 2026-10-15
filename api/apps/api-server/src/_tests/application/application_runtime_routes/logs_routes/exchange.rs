use super::*;
use control_plane_contracts::ports::{AgentLogEvent, AgentLogsBatch, AGENT_LOGS_SCHEMA_VERSION};

async fn request(
    app: &axum::Router,
    id: Uuid,
    operation: &str,
    cookie: Option<&str>,
    csrf: Option<&str>,
    body: Value,
) -> axum::response::Response {
    let mut request = Request::builder()
        .method("POST")
        .uri(format!("/api/console/applications/{id}/logs/{operation}"))
        .header("content-type", "application/json");
    if let Some(cookie) = cookie {
        request = request.header("cookie", cookie);
    }
    if let Some(csrf) = csrf {
        request = request.header("x-csrf-token", csrf);
    }
    app.clone()
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap()
}
async fn value(response: axum::response::Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
}
async fn create(app: &axum::Router, cookie: &str, csrf: &str, kind: &str) -> Uuid {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/console/applications")
                .header("cookie", cookie)
                .header("x-csrf-token", csrf)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"application_type":kind,"name":"Exchange fixture","description":""})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    Uuid::parse_str(value(response).await["data"]["id"].as_str().unwrap()).unwrap()
}
fn batch(source: &str, task: &str, content: &str) -> AgentLogsBatch {
    let event:AgentLogEvent=serde_json::from_value(json!({"event_id":"original-event","source_session_id":"session","source_task_id":task,"sequence":0,"occurred_at":"2026-10-07T00:00:00Z","kind":"context","content":content,"raw":{"unknown":{"array":[true,null,123],"long":"完整原始事实"}}})).unwrap();
    AgentLogsBatch {
        schema_version: AGENT_LOGS_SCHEMA_VERSION.into(),
        source_id: source.into(),
        source_client: "codex".into(),
        events: vec![event],
    }
}
fn import_body(batches: &[AgentLogsBatch]) -> Value {
    json!({"otlp":crate::agent_logs_protocol::batches_to_traces(batches).unwrap()})
}
#[tokio::test]
async fn console_otlp_roundtrip_preserves_multisource_facts_and_null_console_provenance() {
    let (app, database_url) = test_app_with_database_url().await;
    let (cookie, csrf) = login_and_capture_cookie(&app, "root", "change-me").await;
    let id = create(&app, &cookie, &csrf, "agent_logs").await;
    let native = create(&app, &cookie, &csrf, "agent_flow").await;
    let pool = storage_durable_postgres::connect(&database_url)
        .await
        .unwrap();
    let scope: Uuid = sqlx::query_scalar("select scope_id from applications where id=$1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let store = MainDurableStore::new(pool.clone());
    let facts = vec![
        batch("source-a", "task-a", "original\ntext"),
        batch("source-b", "task-b", "context"),
    ];
    let body = import_body(&facts);
    for operation in [
        "otlp/import",
        "otlp/export",
        "langfuse/import",
        "langfuse/export",
    ] {
        let payload = match operation {
            "otlp/import" => body.clone(),
            "otlp/export" => json!({"record_ids":[]}),
            "langfuse/import" => {
                json!({"trace_ids":["trace"],"base_url":"http://localhost","public_key":"pk","secret_key":"sk"})
            }
            _ => {
                json!({"record_ids":[],"base_url":"http://localhost","public_key":"pk","secret_key":"sk"})
            }
        };
        assert_eq!(
            request(&app, id, operation, None, None, payload.clone())
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            request(&app, id, operation, Some(&cookie), None, payload.clone())
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            request(
                &app,
                id,
                operation,
                Some(&cookie),
                Some("invalid"),
                payload.clone()
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            request(&app, native, operation, Some(&cookie), Some(&csrf), payload)
                .await
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    // New Agent Logs applications default to enabled reception. Set the tested
    // disabled state through the canonical writer before asserting the import gate.
    {
        use control_plane_contracts::ports::{
            ApplicationPublicationRepository, ApplicationRepository, SetApplicationApiEnabledInput,
        };
        let application = store.get_application(scope, id).await.unwrap().unwrap();
        store
            .set_application_api_enabled(&SetApplicationApiEnabledInput {
                actor_user_id: application.created_by,
                application_id: id,
                api_enabled: false,
            })
            .await
            .unwrap();
    }
    assert_eq!(
        request(
            &app,
            id,
            "otlp/import",
            Some(&cookie),
            Some(&csrf),
            body.clone()
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    enable_agent_logs_fixture(&store, scope, id).await;
    let response = request(
        &app,
        id,
        "otlp/import",
        Some(&cookie),
        Some(&csrf),
        body.clone(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let receipt = value(response).await;
    assert_eq!(receipt["data"]["accepted_events"], 2);
    let records = receipt["data"]["record_ids"].clone();
    let provenance: Vec<Option<Uuid>> = sqlx::query_scalar(
        "select api_key_id from application_run_log_tasks where application_id=$1",
    )
    .bind(id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(provenance, vec![None, None]);
    let replay =
        value(request(&app, id, "otlp/import", Some(&cookie), Some(&csrf), body).await).await;
    assert_eq!(replay["data"]["accepted_events"], 0);
    assert_eq!(replay["data"]["duplicate_events"], 2);
    // Reception is an import gate. Existing selected facts remain exportable.
    sqlx::query("update applications set api_enabled=false where id=$1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    let response = request(
        &app,
        id,
        "otlp/export",
        Some(&cookie),
        Some(&csrf),
        json!({"record_ids":records}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers()["content-disposition"]
        .to_str()
        .unwrap()
        .contains(".otlp.json"));
    let otlp: crate::agent_logs_protocol::ExportTraceServiceRequest =
        serde_json::from_value(value(response).await).unwrap();
    let restored = crate::agent_logs_protocol::traces_to_batches(&otlp).unwrap();
    assert_eq!(
        serde_json::to_value(restored).unwrap(),
        serde_json::to_value(&facts).unwrap()
    );
    assert_eq!(
        request(
            &app,
            id,
            "otlp/export",
            Some(&cookie),
            Some(&csrf),
            json!({"record_ids":[]})
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(
            &app,
            id,
            "otlp/export",
            Some(&cookie),
            Some(&csrf),
            json!({"record_ids":[Uuid::now_v7()]})
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    // Public ingestion keeps real API key provenance.
    enable_agent_logs_fixture(&store, scope, id).await;
    let key = Uuid::now_v7();
    control_plane::agent_logs::AgentLogsService::new(store.clone())
        .ingest(
            id,
            scope,
            key,
            batch("public-source", "public-task", "public"),
        )
        .await
        .unwrap();
    let public_key:Option<Uuid>=sqlx::query_scalar("select api_key_id from application_run_log_tasks where application_id=$1 and source_id='public-source'").bind(id).fetch_one(&pool).await.unwrap();
    assert_eq!(public_key, Some(key));
}

#[tokio::test]
async fn console_multisource_partial_import_keeps_commits_and_idempotent_retry() {
    let (app, database_url) = test_app_with_database_url().await;
    let (cookie, csrf) = login_and_capture_cookie(&app, "root", "change-me").await;
    let id = create(&app, &cookie, &csrf, "agent_logs").await;
    let pool = storage_durable_postgres::connect(&database_url)
        .await
        .unwrap();
    let scope: Uuid = sqlx::query_scalar("select scope_id from applications where id=$1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let store = MainDurableStore::new(pool.clone());
    enable_agent_logs_fixture(&store, scope, id).await;
    let original = batch("z-existing", "task", "original");
    assert_eq!(
        request(
            &app,
            id,
            "otlp/import",
            Some(&cookie),
            Some(&csrf),
            import_body(&[original.clone()])
        )
        .await
        .status(),
        StatusCode::OK
    );
    let new = batch("a-new", "task", "new");
    let conflict = batch("z-existing", "task", "changed");
    let response = request(
        &app,
        id,
        "otlp/import",
        Some(&cookie),
        Some(&csrf),
        import_body(&[new.clone(), conflict]),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let error = value(response).await;
    assert_eq!(error["code"], "agent_logs_partial_import");
    assert_eq!(error["details"]["accepted_events"], 1);
    assert_eq!(error["details"]["committed_records"], 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "select count(*) from application_run_log_tasks where application_id=$1"
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap(),
        2
    );
    let response = request(
        &app,
        id,
        "otlp/import",
        Some(&cookie),
        Some(&csrf),
        import_body(&[new, original]),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(value(response).await["data"]["duplicate_events"], 2);
}

#[tokio::test]
async fn console_langfuse_gathers_all_pages_before_persistence_and_exports_original_envelope() {
    use axum::{
        body::Bytes,
        extract::State,
        http::{HeaderMap, Uri},
        response::IntoResponse,
        routing::any,
    };
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex},
    };
    type Mock = Arc<Mutex<(VecDeque<Value>, Vec<(String, String, Vec<u8>)>)>>;
    async fn remote(
        State(mock): State<Mock>,
        uri: Uri,
        headers: HeaderMap,
        body: Bytes,
    ) -> impl IntoResponse {
        let mut mock = mock.lock().unwrap();
        mock.1.push((
            uri.to_string(),
            headers["authorization"].to_str().unwrap().into(),
            body.to_vec(),
        ));
        let reply = mock.0.pop_front().expect("unexpected remote call");
        ([("content-type", "application/json")], reply.to_string())
    }
    fn observation(trace: &str, batch: &AgentLogsBatch) -> Value {
        json!({"id":batch.events[0].event_id,"traceId":trace,"metadata":{"1flowbase_agent_log":{"schema_version":batch.schema_version,"source_id":batch.source_id,"source_client":batch.source_client,"event":batch.events[0]}}})
    }
    fn page(items: Vec<Value>, meta: Value) -> Value {
        json!({"data":items,"meta":meta})
    }
    let a = batch("source-a", "task-a", "original first page");
    let mut a2 = batch("source-a", "task-a", "original second page");
    a2.events[0].event_id = "second-event".into();
    a2.events[0].sequence = 1;
    let b = batch("source-b", "task-b", "original next trace");
    let mock: Mock = Arc::new(Mutex::new((
        vec![
            page(vec![observation("trace-a", &a)], json!({})),
            page(vec![observation("wrong-trace", &b)], json!({})),
            page(
                vec![observation("trace-a", &a)],
                json!({"cursor":"page-two"}),
            ),
            page(vec![observation("trace-a", &a2)], json!({})),
            page(vec![observation("trace-b", &b)], json!({})),
            json!({}),
            json!({"partialSuccess":{"rejectedSpans":"1","errorMessage":"REMOTE_PRIVATE_ERROR"}}),
        ]
        .into(),
        vec![],
    )));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let remote_app = axum::Router::new()
        .fallback(any(remote))
        .with_state(mock.clone());
    struct Stop(tokio::task::JoinHandle<()>);
    impl Drop for Stop {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let _server = Stop(tokio::spawn(async move {
        axum::serve(listener, remote_app).await.unwrap();
    }));
    let (app, database_url) = test_app_with_database_url().await;
    let (cookie, csrf) = login_and_capture_cookie(&app, "root", "change-me").await;
    let id = create(&app, &cookie, &csrf, "agent_logs").await;
    let pool = storage_durable_postgres::connect(&database_url)
        .await
        .unwrap();
    let scope: Uuid = sqlx::query_scalar("select scope_id from applications where id=$1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let store = MainDurableStore::new(pool.clone());
    enable_agent_logs_fixture(&store, scope, id).await;
    let body = json!({"trace_ids":["trace-a","trace-b"],"base_url":base,"public_key":"pk","secret_key":"sk"});
    let response = request(
        &app,
        id,
        "langfuse/import",
        Some(&cookie),
        Some(&csrf),
        body.clone(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "select count(*) from application_run_log_tasks where application_id=$1"
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap(),
        0,
        "no first-trace commit on later gathering failure"
    );
    let response = request(
        &app,
        id,
        "langfuse/import",
        Some(&cookie),
        Some(&csrf),
        body,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let receipt = value(response).await;
    assert_eq!(receipt["data"]["accepted_events"], 3);
    let records: Vec<Uuid> = serde_json::from_value(receipt["data"]["record_ids"].clone()).unwrap();
    let facts = store
        .read_imported_agent_logs(id, scope, &records)
        .await
        .unwrap();
    let mut expected = a.clone();
    expected.events.push(a2.events[0].clone());
    assert_eq!(
        serde_json::to_value(facts).unwrap(),
        serde_json::to_value(vec![expected, b]).unwrap()
    );
    let export = json!({"record_ids":records,"base_url":base,"public_key":"pk","secret_key":"sk"});
    let response = request(
        &app,
        id,
        "langfuse/export",
        Some(&cookie),
        Some(&csrf),
        export.clone(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let exported = value(response).await;
    assert_eq!(exported["data"]["exported_records"], 2);
    assert_eq!(exported["data"]["exported_spans"], 3);
    let response = request(
        &app,
        id,
        "langfuse/export",
        Some(&cookie),
        Some(&csrf),
        export,
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert!(!value(response)
        .await
        .to_string()
        .contains("REMOTE_PRIVATE_ERROR"));
    let mock = mock.lock().unwrap();
    assert_eq!(mock.1.len(), 7);
    assert!(mock
        .1
        .iter()
        .all(|(_, authorization, _)| authorization == "Basic cGs6c2s="));
    assert!(mock.1[3].0.contains("cursor=page-two"));
    let sent: crate::agent_logs_protocol::ExportTraceServiceRequest =
        serde_json::from_slice(&mock.1[5].2).unwrap();
    let restored = crate::agent_logs_protocol::traces_to_batches(&sent).unwrap();
    assert_eq!(
        restored
            .iter()
            .map(|batch| batch.events.len())
            .sum::<usize>(),
        3
    );
    assert_eq!(restored[0].events[0].raw, a.events[0].raw);
}

#[tokio::test]
async fn console_conversation_export_includes_all_tasks_and_record_export_remains_exact() {
    use axum::{
        body::Bytes, extract::State, http::HeaderMap, response::IntoResponse, routing::post,
    };
    use control_plane_contracts::ports::{
        ApplicationPublicationRepository, ApplicationRepository, SetApplicationApiEnabledInput,
    };
    use std::sync::{Arc, Mutex};
    type Captures = Arc<Mutex<Vec<Vec<u8>>>>;
    async fn accept(
        State(captures): State<Captures>,
        headers: HeaderMap,
        body: Bytes,
    ) -> impl IntoResponse {
        assert_eq!(headers["authorization"].to_str().unwrap(), "Basic cGs6c2s=");
        captures.lock().unwrap().push(body.to_vec());
        ([("content-type", "application/json")], "{}")
    }
    let captures: Captures = Arc::new(Mutex::new(vec![]));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let remote = axum::Router::new()
        .route("/api/public/otel/v1/traces", post(accept))
        .with_state(captures.clone());
    struct Stop(tokio::task::JoinHandle<()>);
    impl Drop for Stop {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let _server = Stop(tokio::spawn(async move {
        axum::serve(listener, remote).await.unwrap();
    }));
    let (app, database_url) = test_app_with_database_url().await;
    let (cookie, csrf) = login_and_capture_cookie(&app, "root", "change-me").await;
    let id = create(&app, &cookie, &csrf, "agent_logs").await;
    let other = create(&app, &cookie, &csrf, "agent_logs").await;
    let native = create(&app, &cookie, &csrf, "agent_flow").await;
    let pool = storage_durable_postgres::connect(&database_url)
        .await
        .unwrap();
    let scope: Uuid = sqlx::query_scalar("select scope_id from applications where id=$1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let store = MainDurableStore::new(pool.clone());
    enable_agent_logs_fixture(&store, scope, id).await;
    let mut facts = batch("full-chat-source", "task-a", "full original facts");
    facts.events.clear();
    for (task_index, (session, task)) in [
        ("session-a", "task-a"),
        ("session-a", "task-b"),
        ("session-b", "task-c"),
    ]
    .into_iter()
    .enumerate()
    {
        for offset in 0..7 {
            let mut event = batch("full-chat-source", task, "full original facts")
                .events
                .remove(0);
            event.event_id = format!("{task}-{offset}");
            event.source_session_id = session.into();
            event.sequence = (task_index * 7 + offset) as i64;
            event.raw = json!({"task":task,"index":offset,"db.host":"literal","db":{"host":"nested"},"nul":"source\u{0}text"});
            facts.events.push(event);
        }
    }
    let response = request(
        &app,
        id,
        "otlp/import",
        Some(&cookie),
        Some(&csrf),
        import_body(&[facts.clone()]),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(value(response).await["data"]["accepted_events"], 21);
    let conversations:Vec<Uuid> = sqlx::query_scalar("select id from application_conversations where application_id=$1 and source_kind='imported' order by id").bind(id).fetch_all(&pool).await.unwrap();
    assert_eq!(conversations.len(), 2);
    let record:Uuid = sqlx::query_scalar("select id from application_run_log_tasks where application_id=$1 and source_task_id='task-a'").bind(id).fetch_one(&pool).await.unwrap();
    let application = store.get_application(scope, id).await.unwrap().unwrap();
    store
        .set_application_api_enabled(&SetApplicationApiEnabledInput {
            actor_user_id: application.created_by,
            application_id: id,
            api_enabled: false,
        })
        .await
        .unwrap();
    let response = request(
        &app,
        id,
        "otlp/export",
        Some(&cookie),
        Some(&csrf),
        json!({"log_conversation_ids":conversations}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let otlp: crate::agent_logs_protocol::ExportTraceServiceRequest =
        serde_json::from_value(value(response).await).unwrap();
    assert_eq!(
        serde_json::to_value(crate::agent_logs_protocol::traces_to_batches(&otlp).unwrap())
            .unwrap(),
        serde_json::to_value(&[facts.clone()]).unwrap()
    );
    let response = request(
        &app,
        id,
        "otlp/export",
        Some(&cookie),
        Some(&csrf),
        json!({"record_ids":[record]}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let exact: crate::agent_logs_protocol::ExportTraceServiceRequest =
        serde_json::from_value(value(response).await).unwrap();
    let restored = crate::agent_logs_protocol::traces_to_batches(&exact).unwrap();
    assert_eq!(
        serde_json::to_value(&restored[0].events).unwrap(),
        serde_json::to_value(&facts.events[..7]).unwrap()
    );
    for (selector, records, spans, traces) in [
        (json!({"log_conversation_ids":conversations}), 3, 21, 2),
        (json!({"record_ids":[record]}), 1, 7, 1),
    ] {
        let mut body = selector;
        body["base_url"] = json!(base);
        body["public_key"] = json!("pk");
        body["secret_key"] = json!("sk");
        let response = request(
            &app,
            id,
            "langfuse/export",
            Some(&cookie),
            Some(&csrf),
            body,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let receipt = value(response).await;
        assert_eq!(receipt["data"]["exported_records"], records);
        assert_eq!(receipt["data"]["exported_spans"], spans);
        assert_eq!(
            receipt["data"]["trace_ids"].as_array().unwrap().len(),
            traces
        );
    }
    let sent = captures.lock().unwrap();
    assert_eq!(sent.len(), 2);
    let sent_all: crate::agent_logs_protocol::ExportTraceServiceRequest =
        serde_json::from_slice(&sent[0]).unwrap();
    assert_eq!(
        serde_json::to_value(crate::agent_logs_protocol::traces_to_batches(&sent_all).unwrap())
            .unwrap(),
        serde_json::to_value(&[facts]).unwrap()
    );
    drop(sent);
    for operation in ["otlp/export", "langfuse/export"] {
        for mut selector in [
            json!({}),
            json!({"record_ids":[]}),
            json!({"log_conversation_ids":[]}),
            json!({"record_ids":[record],"log_conversation_ids":conversations}),
            json!({"record_ids":[],"log_conversation_ids":conversations}),
            json!({"record_ids":[record],"log_conversation_ids":[]}),
        ] {
            if operation == "langfuse/export" {
                selector["base_url"] = json!(base);
                selector["public_key"] = json!("pk");
                selector["secret_key"] = json!("sk");
            }
            assert_eq!(
                request(&app, id, operation, Some(&cookie), Some(&csrf), selector)
                    .await
                    .status(),
                StatusCode::BAD_REQUEST
            );
        }
    }
    assert_eq!(
        captures.lock().unwrap().len(),
        2,
        "invalid selectors never reach remote export"
    );
    assert_eq!(
        request(
            &app,
            other,
            "otlp/export",
            Some(&cookie),
            Some(&csrf),
            json!({"log_conversation_ids":conversations})
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &app,
            native,
            "otlp/export",
            Some(&cookie),
            Some(&csrf),
            json!({"log_conversation_ids":conversations})
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(
            &app,
            id,
            "otlp/export",
            Some(&cookie),
            Some(&csrf),
            json!({"log_conversation_ids":[conversations[0],Uuid::now_v7()]})
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
}

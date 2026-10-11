use super::*;

async fn console(
    app: &Router,
    method: &str,
    uri: &str,
    cookie: Option<&str>,
    csrf: Option<&str>,
    body: Value,
) -> axum::response::Response {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
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
async fn upload(app: &Router, token: &str, event_id: &str) -> axum::response::Response {
    app.clone().oneshot(Request::builder().method("POST").uri("/api/logs/v1/events")
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from(json!({"schema_version":"1flowbase.agent-logs/v1", "source_id":"reception-test", "source_client":"codex", "events":[{
            "event_id":event_id,"source_session_id":"session","source_task_id":"turn","sequence":if event_id == "first" {1} else {2},
            "occurred_at":"2026-10-11T00:00:00Z","kind":"user","content":"reception test"
        }]}).to_string())).unwrap()).await.unwrap()
}

#[tokio::test]
async fn agent_logs_reception_controls_writes_without_publication_or_key_revocation() {
    let (app, database_url) = test_app_with_database_url().await;
    let pool = PgPool::connect(&database_url).await.unwrap();
    let (cookie, csrf) = login_and_capture_cookie(&app, "root", "change-me").await;
    let id = create_application_with_type(&app, &cookie, &csrf, "Reception", "agent_logs").await;
    let status_uri = format!("/api/console/applications/{id}/api-status");
    let key_uri = format!("/api/console/applications/{id}/api-keys");
    assert_eq!(
        console(
            &app,
            "PATCH",
            &status_uri,
            Some(&cookie),
            Some(&csrf),
            json!({"api_enabled":false}),
        )
        .await
        .status(),
        StatusCode::OK
    );
    let key = console(
        &app,
        "POST",
        &key_uri,
        Some(&cookie),
        Some(&csrf),
        json!({"name":"before enabling"}),
    )
    .await;
    assert_eq!(
        key.status(),
        StatusCode::CREATED,
        "keys can be prepared while disabled"
    );
    let token = response_json(key).await["data"]["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let disabled = upload(&app, &token, "first").await;
    assert_eq!(disabled.status(), StatusCode::FORBIDDEN);
    assert!(response_json(disabled)
        .await
        .to_string()
        .contains("agent_logs_ingestion_disabled"));
    let counts = || {
        sqlx::query_scalar::<_, i64>(
            "select count(*) from application_log_upload_receipts where application_id=$1",
        )
        .bind(Uuid::parse_str(&id).unwrap())
        .fetch_one(&pool)
    };
    assert_eq!(counts().await.unwrap(), 0);

    assert_eq!(
        console(
            &app,
            "PATCH",
            &status_uri,
            None,
            None,
            json!({"api_enabled":true})
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        console(
            &app,
            "PATCH",
            &status_uri,
            Some(&cookie),
            None,
            json!({"api_enabled":true})
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    let (viewer_cookie, viewer_csrf) =
        create_member_with_permissions(&app, &cookie, &csrf, &["application.view.all"]).await;
    assert_eq!(
        console(
            &app,
            "PATCH",
            &status_uri,
            Some(&viewer_cookie),
            Some(&viewer_csrf),
            json!({"api_enabled":true})
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );

    let enabled = console(
        &app,
        "PATCH",
        &status_uri,
        Some(&cookie),
        Some(&csrf),
        json!({"api_enabled":true}),
    )
    .await;
    assert_eq!(enabled.status(), StatusCode::OK);
    assert_eq!(
        response_json(enabled).await["data"]["public_url"],
        "/api/logs/v1/events"
    );
    let detail = console(
        &app,
        "GET",
        &format!("/api/console/applications/{id}"),
        Some(&cookie),
        None,
        json!(null),
    )
    .await;
    assert_eq!(
        response_json(detail).await["data"]["sections"]["api"]["api_capability_status"],
        "enabled"
    );
    let first = upload(&app, &token, "first").await;
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(response_json(first).await["data"]["accepted_events"], 1);
    assert_eq!(counts().await.unwrap(), 1);
    let publication_count: i64 = sqlx::query_scalar(
        "select count(*) from application_publication_versions where application_id=$1",
    )
    .bind(Uuid::parse_str(&id).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(publication_count, 0);

    let disabled = console(
        &app,
        "PATCH",
        &status_uri,
        Some(&cookie),
        Some(&csrf),
        json!({"api_enabled":false}),
    )
    .await;
    assert_eq!(disabled.status(), StatusCode::OK);
    assert_eq!(
        upload(&app, &token, "second").await.status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        counts().await.unwrap(),
        1,
        "disabled reception must not create receipts"
    );
    let key = console(
        &app,
        "POST",
        &key_uri,
        Some(&cookie),
        Some(&csrf),
        json!({"name":"while stopped"}),
    )
    .await;
    assert_eq!(key.status(), StatusCode::CREATED);
    let detail = console(
        &app,
        "GET",
        &format!("/api/console/applications/{id}"),
        Some(&cookie),
        None,
        json!(null),
    )
    .await;
    assert_eq!(
        response_json(detail).await["data"]["sections"]["api"]["api_capability_status"],
        "disabled"
    );
    let list = console(
        &app,
        "GET",
        "/api/console/settings/applications",
        Some(&cookie),
        None,
        json!(null),
    )
    .await;
    let list = response_json(list).await;
    let row = list["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == id)
        .unwrap();
    assert_eq!(row["publication_status"], "not_applicable");
    assert_eq!(row["api_enabled"], false);

    assert_eq!(
        console(
            &app,
            "PATCH",
            &status_uri,
            Some(&cookie),
            Some(&csrf),
            json!({"api_enabled":true})
        )
        .await
        .status(),
        StatusCode::OK
    );
    let replay = upload(&app, &token, "first").await;
    assert_eq!(replay.status(), StatusCode::OK);
    assert_eq!(
        response_json(replay).await["data"]["duplicate_events"],
        1,
        "original key stays usable"
    );
    assert_eq!(
        upload(&app, &token, "second").await.status(),
        StatusCode::OK
    );
    assert_eq!(counts().await.unwrap(), 2);
}

#[tokio::test]
async fn agent_logs_reception_is_enabled_on_creation_without_changing_other_types() {
    let (app, database_url) = test_app_with_database_url().await;
    let pool = PgPool::connect(&database_url).await.unwrap();
    let (cookie, csrf) = login_and_capture_cookie(&app, "root", "change-me").await;
    let created = console(
        &app,
        "POST",
        "/api/console/applications",
        Some(&cookie),
        Some(&csrf),
        json!({"application_type":"agent_logs","name":"Default reception","description":""}),
    )
    .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let created = response_json(created).await;
    assert_eq!(
        created["data"]["sections"]["api"]["api_capability_status"],
        "enabled"
    );
    let id = created["data"]["id"].as_str().unwrap();
    let key = console(
        &app,
        "POST",
        &format!("/api/console/applications/{id}/api-keys"),
        Some(&cookie),
        Some(&csrf),
        json!({"name":"Ready after creation"}),
    )
    .await;
    assert_eq!(key.status(), StatusCode::CREATED);
    let key = response_json(key).await;
    let accepted = upload(&app, key["data"]["token"].as_str().unwrap(), "first").await;
    assert_eq!(accepted.status(), StatusCode::OK);
    assert_eq!(response_json(accepted).await["data"]["accepted_events"], 1);

    let flow = create_application(&app, &cookie, &csrf, "Default flow").await;
    let workflow = create_extension_workflow(&app, &cookie, &csrf, "Default workflow").await;
    for (application_id, expected) in [
        (id, true),
        (flow.as_str(), false),
        (workflow.as_str(), false),
    ] {
        let enabled: bool = sqlx::query_scalar("select api_enabled from applications where id=$1")
            .bind(Uuid::parse_str(application_id).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(enabled, expected);
    }
    let publications: i64 = sqlx::query_scalar(
        "select count(*) from application_publication_versions where application_id=$1",
    )
    .bind(Uuid::parse_str(id).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(publications, 0);
}

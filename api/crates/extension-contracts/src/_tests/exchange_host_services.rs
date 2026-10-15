use crate::*;
#[test]
fn exchange_host_services_reject_worker_identity_and_invalid_selectors() {
    let forged = serde_json::json!({"operation":"list_applications","application_types":[],"workspace_id":"system"});
    assert!(serde_json::from_value::<ApplicationLogsRequest>(forged).is_err());
    for (record_ids, log_conversation_ids) in [
        (None, None),
        (Some(vec![]), None),
        (Some(vec!["r".into()]), Some(vec!["c".into()])),
    ] {
        assert!(ApplicationLogsRequest::ExportOtlp {
            application_id: "a".into(),
            record_ids,
            log_conversation_ids
        }
        .validate()
        .is_err());
    }
    let request = ApplicationLogsRequest::ExportOtlp {
        application_id: "a".into(),
        record_ids: Some(vec!["r".into()]),
        log_conversation_ids: None,
    };
    assert!(request.validate().is_ok());
    assert_eq!(request.permission(), "application.logs.read");
    assert_eq!(
        ApplicationLogsRequest::ImportOtlp {
            application_id: "a".into(),
            otlp: serde_json::json!({})
        }
        .permission(),
        "application.logs.import"
    );
}
#[test]
fn exchange_host_services_preserve_partial_receipts_and_raw_http_status() {
    let error = HostServiceError {
        code: "agent_logs_partial_import".into(),
        status: Some(409),
        details: Some(serde_json::json!({"accepted_events":4,"duplicate_events":2})),
    };
    assert_eq!(
        serde_json::from_value::<HostServiceError>(serde_json::to_value(&error).unwrap()).unwrap(),
        error
    );
    let http = HttpEgressResponse {
        status: 429,
        headers: Default::default(),
        body_base64: "cmV0cnk=".into(),
    };
    assert_eq!(
        serde_json::from_value::<HttpEgressResponse>(serde_json::to_value(&http).unwrap()).unwrap(),
        http
    );
    for url in [
        "file:///etc/passwd",
        "https://user:secret@example.com/",
        "https://example.com/#secret",
    ] {
        assert!(HttpEgressRequest {
            method: "GET".into(),
            url: url.into(),
            headers: Default::default(),
            body_base64: None
        }
        .validate()
        .is_err());
    }
}

#[test]
fn exchange_host_services_http_wire_rejects_worker_identity() {
    let forged = serde_json::json!({"method":"GET","url":"https://example.com/","headers":{},"body_base64":null,"installation_id":"forged"});
    assert!(serde_json::from_value::<HttpEgressRequest>(forged).is_err());
}

#[test]
fn exchange_host_services_preserve_absent_native_conversation_relation() {
    let record: super::ApplicationLogsRecord = serde_json::from_value(serde_json::json!({
        "record_id": "native-run", "log_conversation_id": null,
        "title": "Native history", "started_at": "2026-10-15T00:00:00Z", "source_kind": "native"
    })).unwrap();
    assert_eq!(record.log_conversation_id, None);
    assert!(serde_json::to_value(record).unwrap()["log_conversation_id"].is_null());
}

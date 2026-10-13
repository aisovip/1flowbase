use std::sync::atomic::{AtomicUsize, Ordering};

use crate::{execute_notification, CapabilityStdioRequest, NotificationTypeDescriptor};
use serde_json::{json, Value};

fn descriptor() -> NotificationTypeDescriptor {
    let manifest: Value = serde_json::from_str(include_str!(
        "../../../../plugins/fixtures/acme.notification/manifest.yaml"
    ))
    .unwrap();
    serde_json::from_value(manifest["notification_types"][0].clone()).unwrap()
}

fn request(text: Value) -> CapabilityStdioRequest {
    serde_json::from_value(json!({"method":"execute","input":{"contribution_code":"send","config_payload":{"token":"private-token","channel":"ops"},"input_payload":{"text":text}}})).unwrap()
}

#[test]
fn notification_sdk_keeps_config_separate_and_projects_only_declared_output() {
    let response = execute_notification(
        request(json!("hello")),
        &descriptor(),
        |config, input| -> Result<Value, String> {
            assert_eq!(config["token"], "private-token");
            assert!(input.get("token").is_none());
            Ok(
                json!({"accepted":true,"delivery_id":format!("{}:{}",config["channel"].as_str().unwrap(),input["text"].as_str().unwrap())}),
            )
        },
    );
    assert!(response.ok);
    assert_eq!(
        response.result,
        json!({"accepted":true,"delivery_id":"ops:hello"})
    );
    assert!(!serde_json::to_string(&response)
        .unwrap()
        .contains("private-token"));
}

#[test]
fn notification_sdk_rejects_invalid_input_output_and_redacts_provider_errors() {
    let calls = AtomicUsize::new(0);
    let response = execute_notification(
        request(json!(42)),
        &descriptor(),
        |_, _| -> Result<Value, String> {
            calls.fetch_add(1, Ordering::SeqCst);
            unreachable!("invalid input must never reach the provider")
        },
    );
    assert!(!response.ok);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    for result in [
        Ok(json!({"accepted":"yes","delivery_id":"bad"})),
        Err("private-token".to_owned()),
    ] {
        let response = execute_notification(request(json!("hello")), &descriptor(), |_, _| result);
        assert!(!response.ok);
        assert!(response.result.is_null());
        assert!(!serde_json::to_string(&response)
            .unwrap()
            .contains("private-token"));
    }
}

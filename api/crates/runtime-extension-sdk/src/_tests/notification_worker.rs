//! Actual process fixture using the SDK and existing capability wire envelope.
use runtime_extension_sdk::{
    execute_notification, CapabilityStdioRequest, NotificationTypeDescriptor,
};
use serde_json::{json, Value};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let request: CapabilityStdioRequest = serde_json::from_reader(std::io::stdin().lock())?;
    let manifest: Value = serde_json::from_str(include_str!(
        "../../../../plugins/fixtures/acme.notification/manifest.yaml"
    ))?;
    let descriptor: NotificationTypeDescriptor =
        serde_json::from_value(manifest["notification_types"][0].clone())?;
    let response = if request.input["input_payload"]["text"] == "fixture-invalid-output" {
        // Deliberately bypass author-side validation to prove the Host rejects a bad peer.
        json!({"ok":true,"result":{"accepted":"yes","delivery_id":"invalid"}})
    } else {
        serde_json::to_value(execute_notification(
            request,
            &descriptor,
            |config, input| -> Result<Value, String> {
                if input["text"] == "fixture-provider-error" {
                    return Err(format!("failed with token {}", config["token"]));
                }
                let delivery_id = if input["text"] == "fixture-secret-output" {
                    config["token"].as_str().unwrap().to_owned()
                } else {
                    format!(
                        "{}:{}",
                        config["channel"].as_str().unwrap(),
                        input["text"].as_str().unwrap()
                    )
                };
                Ok(json!({"accepted":true,"delivery_id":delivery_id}))
            },
        ))?
    };
    serde_json::to_writer(std::io::stdout().lock(), &response)?;
    Ok(())
}

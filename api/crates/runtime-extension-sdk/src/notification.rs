//! Notification handlers reuse the capability envelope and the shared worker dispatcher.
use extension_contracts::{
    CapabilityStdioError, CapabilityStdioMethod, CapabilityStdioRequest, CapabilityStdioResponse,
    NotificationTypeDescriptor,
};
use serde_json::Value;

/// Apply one provider send without owning transport, workspace policy or process lifecycle.
/// Provider errors are projected to a safe message; configuration never enters a response.
pub fn execute_notification<E>(
    request: CapabilityStdioRequest,
    descriptor: &NotificationTypeDescriptor,
    handler: impl FnOnce(&Value, &Value) -> Result<Value, E>,
) -> CapabilityStdioResponse {
    fn failed(message: &str) -> CapabilityStdioResponse {
        CapabilityStdioResponse {
            ok: false,
            result: Value::Null,
            error: Some(CapabilityStdioError {
                message: message.into(),
                provider_summary: None,
            }),
        }
    }
    if request.method != CapabilityStdioMethod::Execute {
        return failed("notification requires the execute method");
    }
    let config = &request.input["config_payload"];
    let input = &request.input["input_payload"];
    if descriptor.validate_config(config).is_err() || descriptor.validate_input(input).is_err() {
        return failed("notification configuration or input violates its schema");
    }
    match handler(config, input) {
        Ok(result) if descriptor.validate_output(&result).is_ok() => CapabilityStdioResponse {
            ok: true,
            result,
            error: None,
        },
        Ok(_) => failed("notification output violates its schema"),
        Err(_) => failed("notification provider send failed"),
    }
}

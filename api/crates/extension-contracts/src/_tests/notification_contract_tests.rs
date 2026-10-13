use crate::{ContributionId, NotificationTypeDescriptor, PluginFormSchema};
use serde_json::json;

fn descriptor() -> NotificationTypeDescriptor {
    NotificationTypeDescriptor {
        type_code: "acme/fixture/webhook".into(),
        display_name: "Webhook".into(),
        description: "Sends a provider payload".into(),
        config_schema: json!({"type":"object","properties":{"token":{"type":"string","minLength":1}},"required":["token"],"additionalProperties":false}),
        config_form: serde_json::from_value(json!({"schema_version":"1.0.0","fields":[{"key":"token","label":"Token","type":"string","control":"password"}]})).unwrap(),
        input_schema: json!({"type":"object","properties":{"text":{"type":"string"}},"required":["text"],"additionalProperties":false}),
        input_form: serde_json::from_value(json!({"schema_version":"1.0.0","fields":[{"key":"text","label":"Text","type":"string"}]})).unwrap(),
        output_schema: json!({"type":"object","properties":{"accepted":{"type":"boolean"}},"required":["accepted"],"additionalProperties":false}),
        contribution_id: ContributionId::new("fixture.send").unwrap(),
    }
}

#[test]
fn notification_config_input_output_are_independent_and_errors_do_not_reveal_secrets() {
    let descriptor = descriptor();
    descriptor
        .validate_plugin_namespace("acme", "fixture")
        .unwrap();
    assert_eq!(
        descriptor.secret_config_fields().collect::<Vec<_>>(),
        ["token"]
    );
    descriptor
        .validate_config(&json!({"token":"credential"}))
        .unwrap();
    descriptor
        .validate_input(&json!({"text":"message"}))
        .unwrap();
    descriptor
        .validate_output(&json!({"accepted":true}))
        .unwrap();
    assert!(descriptor
        .validate_config(&json!({"text":"message"}))
        .is_err());
    assert!(descriptor
        .validate_input(&json!({"token":"credential"}))
        .is_err());
    let error = descriptor
        .validate_config(&json!({"token":0,"extra":"credential"}))
        .unwrap_err();
    assert!(!error.to_string().contains("credential"));
    assert!(descriptor
        .validate_output(&json!({"accepted":"true"}))
        .is_err());
}

#[test]
fn notification_descriptor_rejects_invalid_identity_schema_and_form() {
    let original = descriptor();
    for type_code in [
        "system",
        "other/fixture/webhook",
        "acme/fixture/",
        "acme/fixture/a/b",
    ] {
        let mut value = original.clone();
        value.type_code = type_code.into();
        assert!(value.validate_plugin_namespace("acme", "fixture").is_err());
    }
    for schema in [
        json!({"type":"unknown"}),
        json!({"$ref":"https://example.com/schema"}),
        json!({"type":"object","properties":{"x":{"$dynamicRef":"file:///secret"}}}),
    ] {
        let mut value = original.clone();
        value.output_schema = schema;
        assert!(value.validate().is_err());
    }
    let mut value = original.clone();
    value.config_form.fields[0].key = "missing".into();
    assert!(value.validate().is_err());
    let mut value = original.clone();
    value.input_form.fields[0].control = Some("password".into());
    assert!(value.validate().is_err());
    let mut value = original.clone();
    value
        .config_form
        .fields
        .push(value.config_form.fields[0].clone());
    assert!(value.validate().is_err());
    let mut value = original;
    value.config_form = PluginFormSchema {
        schema_version: "unknown".into(),
        title: None,
        description: None,
        fields: vec![],
    };
    assert!(value.validate().is_err());
}

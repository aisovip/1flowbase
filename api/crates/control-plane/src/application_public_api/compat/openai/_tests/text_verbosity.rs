use super::*;
use crate::application_public_api::native::ResponsesTransportRequirement;

#[test]
fn verbosity_is_a_reported_style_hint_with_exact_preferred_wire() {
    for verbosity in ["low", "medium", "high"] {
        let body = json!({"model":"fixture", "input":"hello",
            "instructions":"Answer in Chinese.", "text":{"verbosity":verbosity}});
        let mut translated = translate_response_request(body.clone()).unwrap();
        assert_eq!(
            responses_transport_requirement(body.as_object().unwrap()),
            ResponsesTransportRequirement::SemanticCompatible
        );
        let system = translated
            .request
            .system
            .iter()
            .map(|block| block.text_content())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(system.contains("Answer in Chinese."));
        assert!(system.contains(&format!("verbosity preference: {verbosity}")));
        assert!(translated
            .report
            .decisions
            .iter()
            .any(|decision| decision.source_path == "$.text.verbosity"
                && decision.kind == TranslationDecisionKind::Normalized
                && decision.target_path.as_deref() == Some("$.system")));
        assert_eq!(
            translated
                .request
                .metadata
                .take_provider_transport_payload()
                .unwrap()
                .wire_body(),
            &body
        );
        translated.report.ensure_consistent().unwrap();
    }
}

#[test]
fn unsupported_text_semantics_stay_native_without_a_prompt_approximation() {
    for text in [
        json!({"verbosity":"low", "format":{"type":"json_schema"}}),
        json!({"verbosity":"high", "future_option":true}),
    ] {
        let body = json!({"model":"fixture", "input":"hello", "text":text});
        assert_eq!(
            responses_transport_requirement(body.as_object().unwrap()),
            ResponsesTransportRequirement::NativePassthrough
        );
        let mut translated = translate_response_request(body.clone()).unwrap();
        assert!(translated.request.system.is_empty());
        assert_eq!(
            translated
                .request
                .metadata
                .take_provider_transport_payload()
                .unwrap()
                .wire_body(),
            &body
        );
    }
}

#[test]
fn invalid_verbosity_is_rejected_and_empty_text_does_not_add_instructions() {
    for verbosity in [json!("huge"), json!(17), json!({})] {
        let error = translate_response_request(json!({"model":"fixture", "input":"hello",
            "text":{"verbosity":verbosity}}))
        .unwrap_err();
        assert_eq!(error.param.as_deref(), Some("text.verbosity"));
        assert_eq!(error.code, "invalid_request");
    }
    for text in [Value::Null, json!({}), json!({"verbosity":null})] {
        let translated =
            translate_response_request(json!({"model":"fixture", "input":"hello", "text":text}))
                .unwrap();
        assert!(translated.request.system.is_empty());
    }
}

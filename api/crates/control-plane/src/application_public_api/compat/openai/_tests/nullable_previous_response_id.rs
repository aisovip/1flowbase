use serde_json::json;

use super::responses_index::{ResponsesInputIndexError, ResponsesRequestIndex};
use super::OpenAiResponsesEnvelope;

#[test]
fn nullable_previous_response_id_accepts_absent_and_null_without_rewriting_body() {
    for request in [
        json!({"model":"gpt-6-luna","input":"full context"}),
        json!({"model":"gpt-6-luna","input":"full context","previous_response_id":null}),
    ] {
        let envelope = OpenAiResponsesEnvelope::capture(request.clone())
            .expect("an absent or null cursor starts a full Responses request");
        assert_eq!(envelope.previous_response_id(), None);
        assert_eq!(envelope.raw_body(), &request);
        assert_eq!(envelope.provider_transport_payload().wire_body(), &request);
    }
}

#[test]
fn nullable_previous_response_id_preserves_nonempty_continuation() {
    let request = json!({"input":[],"previous_response_id":"resp_previous"});
    let envelope = OpenAiResponsesEnvelope::capture(request.clone()).unwrap();
    assert_eq!(envelope.previous_response_id(), Some("resp_previous"));
    assert_eq!(envelope.raw_body(), &request);
}

#[test]
fn nullable_previous_response_id_rejects_empty_and_wrong_types() {
    for cursor in [json!(""), json!(42), json!(true), json!([]), json!({})] {
        let request = json!({"input":[],"previous_response_id":cursor});
        assert_eq!(
            ResponsesRequestIndex::build(&request),
            Err(ResponsesInputIndexError::PreviousResponseId)
        );
        let error = OpenAiResponsesEnvelope::capture(request).unwrap_err();
        assert_eq!(error.param.as_deref(), Some("previous_response_id"));
    }
}

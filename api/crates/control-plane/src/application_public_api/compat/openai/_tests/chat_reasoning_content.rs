use super::*;

#[test]
fn chat_reasoning_content_dsh_resume_preserves_tools_and_visible_text() {
    // Real DSH resume shape: provider replay restores reasoning_content on
    // historical assistant tool-call messages, followed by a new user message.
    let translated = translate_chat_completion_request(json!({
        "model":"gpt-6-luna", "reasoning_effort":"max", "stream":true,
        "messages":[
            {"role":"user","content":"read the task-owned file"},
            {"role":"assistant","content":null,"reasoning_content":"private tool reasoning",
                "tool_calls":[{"id":"call_read","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"seed.txt\"}"}}]},
            {"role":"tool","tool_call_id":"call_read","content":"42"},
            {"role":"assistant","content":"FIRST_OK","reasoning_content":"private final reasoning"},
            {"role":"user","content":"continue and verify"}
        ]
    })).expect("native DSH assistant reasoning must survive Chat resume");
    let history = &translated.request.history;
    assert_eq!(translated.request.query, "continue and verify");
    assert_eq!(history[1]["content"], "");
    assert_eq!(
        history[1]["content_blocks"],
        json!([{"type":"reasoning","text":"private tool reasoning"}])
    );
    assert_eq!(history[1]["tool_calls"][0]["id"], "call_read");
    assert_eq!(history[1]["tool_calls"][0]["name"], "read_file");
    assert_eq!(history[2]["tool_call_id"], "call_read");
    assert_eq!(history[2]["content"], "42");
    assert_eq!(history[3]["content"], "FIRST_OK");
    assert_eq!(
        history[3]["content_blocks"],
        json!([
            {"type":"reasoning","text":"private final reasoning"},
            {"type":"text","text":"FIRST_OK"}
        ])
    );
    assert!(translated.report.has_decision(
        "$.messages[1].reasoning_content",
        TranslationDecisionKind::Normalized
    ));
    let receipt = serde_json::to_string(&translated.report).unwrap();
    assert!(!receipt.contains("private tool reasoning"));
    assert!(!receipt.contains("private final reasoning"));
}

#[test]
fn chat_reasoning_content_preserves_media_blocks_and_literal_text() {
    let reasoning = "draft\0after\\u0000";
    let translated = translate_chat_completion_request(json!({"model":"gpt-6-luna","messages":[
        {"role":"assistant","reasoning_content":reasoning,"content":[
            {"type":"text","text":"visible"},
            {"type":"image_url","image_url":{"url":"https://example.com/fixture.png"}}
        ]},
        {"role":"user","content":"continue"}
    ]}))
    .unwrap();
    let history = &translated.request.history[0];
    assert_eq!(history["content"], "visible");
    assert_eq!(
        history["content_blocks"][0],
        json!({"type":"reasoning","text":reasoning})
    );
    assert_eq!(
        history["content_blocks"][1],
        json!({"type":"text","text":"visible"})
    );
    assert_eq!(history["content_blocks"][2]["type"], "image_url");
    assert_eq!(history["content_blocks"].as_array().unwrap().len(), 3);
}

#[test]
fn chat_reasoning_content_null_is_absent_but_empty_string_remains_explicit() {
    let baseline = json!({"model":"gpt-6-luna","messages":[
        {"role":"assistant","content":"visible"}, {"role":"user","content":"next"}
    ]});
    let absent = translate_chat_completion_request(baseline.clone()).unwrap();
    let mut null = baseline.clone();
    null["messages"][0]["reasoning_content"] = Value::Null;
    let translated = translate_chat_completion_request(null).unwrap();
    assert_eq!(translated.request.history, absent.request.history);
    assert!(translated.report.has_decision(
        "$.messages[0].reasoning_content",
        TranslationDecisionKind::Dropped
    ));
    let mut empty = baseline;
    empty["messages"][0]["reasoning_content"] = json!("");
    let translated = translate_chat_completion_request(empty).unwrap();
    assert_eq!(translated.request.history[0]["content"], "visible");
    assert_eq!(
        translated.request.history[0]["content_blocks"][0],
        json!({"type":"reasoning","text":""})
    );
}

#[test]
fn chat_reasoning_content_rejects_non_assistant_roles_even_when_null() {
    for role in ["system", "developer", "user", "tool"] {
        for reasoning in [json!("private"), Value::Null] {
            let error = translate_chat_completion_request(json!({"model":"gpt-6-luna","messages":[
                {"role":role,"content":"text","reasoning_content":reasoning,"tool_call_id":"call_read"},
                {"role":"user","content":"next"}
            ]})).expect_err("reasoning_content belongs only to assistant history");
            assert_eq!(error.code, "invalid_request");
            assert_eq!(error.param.as_deref(), Some("messages"));
            assert!(error.report.has_decision(
                "$.messages[0].reasoning_content",
                TranslationDecisionKind::Rejected
            ));
        }
    }
}

#[test]
fn chat_reasoning_content_rejects_non_text_types_before_tool_content_shortcut() {
    for reasoning in [json!(true), json!(4), json!([]), json!({"text":"private"})] {
        let error = translate_chat_completion_request(json!({"model":"gpt-6-luna","messages":[
            {"role":"assistant","content":null,"reasoning_content":reasoning,
                "tool_calls":[{"id":"call_read","type":"function","function":{"name":"read_file","arguments":"{}"}}]},
            {"role":"user","content":"next"}
        ]})).expect_err("assistant reasoning_content must be text or null");
        assert_eq!(error.code, "invalid_request");
        assert!(error.report.has_decision(
            "$.messages[0].reasoning_content",
            TranslationDecisionKind::Rejected
        ));
    }
}

#[test]
fn chat_reasoning_content_does_not_allow_other_unknown_fields_or_aliases() {
    for field in ["reasoning", "future_message_field"] {
        let mut body = json!({"model":"gpt-6-luna","messages":[
            {"role":"assistant","content":"visible","reasoning_content":"private"},
            {"role":"user","content":"next"}
        ]});
        body["messages"][0][field] = json!("unowned");
        let error =
            translate_chat_completion_request(body).expect_err("unowned fields remain rejected");
        assert_eq!(error.message, "unknown OpenAI Chat message field");
        assert_eq!(error.code, "invalid_request");
    }
}

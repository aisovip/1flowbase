use super::*;

fn notice(label: &str) -> Value {
    let context = ProviderExternalContext::from_responses_notification(&json!({
        "type":"function_call_output", "call_id":null,
        "name":"send_message_to_thread", "namespace":"codex_app",
        "output":{"marker":label}, "trace":{"source":label}
    }))
    .unwrap();
    json!({"role":"user", "content":"", "external_context":context})
}

fn calls(ids: &[&str]) -> Value {
    json!({"role":"assistant", "content":"", "tool_calls":ids.iter().map(|id|
        json!({"id":id,"name":"echo","arguments":{}})).collect::<Vec<_>>()})
}

fn receipt(id: &str) -> Value {
    json!({"role":"tool", "tool_call_id":id, "content":format!("RESULT_{id}")})
}

#[test]
fn interleaved_notifications_preserve_order_without_interrupting_tool_receipts() {
    let original = vec![
        notice("outside_before"),
        calls(&["a", "b"]),
        notice("before"),
        receipt("a"),
        notice("between"),
        receipt("b"),
        notice("after"),
    ];
    let (projected, promotions) = project_tool_round_notifications(original.clone()).unwrap();
    assert_eq!(
        &projected[..4],
        &[
            original[0].clone(),
            original[1].clone(),
            receipt("a"),
            receipt("b")
        ]
    );
    assert_eq!(projected.last(), original.last());
    assert_eq!(promotions.len(), 1);
    assert_eq!(projected[4]["role"], "user");
    assert!(projected[4].get("tool_call_id").is_none());
    let timeline: Value = serde_json::from_str(
        projected[4]["content"]
            .as_str()
            .unwrap()
            .split_once('\n')
            .unwrap()
            .1,
    )
    .unwrap();
    assert_eq!(
        timeline,
        json!([
            {"type":"external_context","source_message_index":2,"context":original[2]["external_context"]},
            {"type":"tool_result_reference","source_message_index":3,"tool_call_id":"a"},
            {"type":"external_context","source_message_index":4,"context":original[4]["external_context"]},
            {"type":"tool_result_reference","source_message_index":5,"tool_call_id":"b"}
        ])
    );
    let context = provider_context_from_prompt_messages(original).unwrap();
    assert_eq!(
        context.messages.iter().map(|m| &m.role).collect::<Vec<_>>(),
        vec![
            &ProviderMessageRole::User,
            &ProviderMessageRole::Assistant,
            &ProviderMessageRole::Tool,
            &ProviderMessageRole::Tool,
            &ProviderMessageRole::User,
            &ProviderMessageRole::User
        ]
    );
    assert_eq!(context.messages[2].content, "RESULT_a");
    assert_eq!(context.messages[3].content, "RESULT_b");
    assert!(context.system.is_empty());
}

#[test]
fn projection_does_not_repair_incomplete_duplicate_or_unrelated_receipts() {
    for input in [
        vec![calls(&["a", "b"]), notice("n"), receipt("a")],
        vec![
            calls(&["a", "b"]),
            notice("n"),
            receipt("a"),
            receipt("a"),
            receipt("b"),
        ],
        vec![calls(&["a"]), notice("n"), receipt("other")],
        vec![
            calls(&["a"]),
            notice("n"),
            json!({"role":"user","content":"new turn"}),
            receipt("a"),
        ],
    ] {
        let (output, promotions) = project_tool_round_notifications(input.clone()).unwrap();
        assert_eq!(output, input);
        assert!(promotions.is_empty());
    }
}

#[test]
fn ordinary_tool_rounds_and_trailing_notifications_are_unchanged() {
    for input in [
        vec![calls(&["a"]), receipt("a")],
        vec![calls(&["a"]), receipt("a"), notice("after")],
        vec![notice("standalone")],
    ] {
        let (output, promotions) = project_tool_round_notifications(input.clone()).unwrap();
        assert_eq!(output, input);
        assert!(promotions.is_empty());
    }
}

#[test]
fn separate_completed_rounds_have_separate_notification_timelines() {
    let (output, promotions) = project_tool_round_notifications(vec![
        calls(&["a"]),
        notice("first"),
        receipt("a"),
        calls(&["b"]),
        notice("second"),
        receipt("b"),
    ])
    .unwrap();
    assert_eq!(promotions.len(), 2);
    assert_eq!(output[1], receipt("a"));
    assert_eq!(output[4], receipt("b"));
    assert!(output[2]["content"].as_str().unwrap().contains("first"));
    assert!(!output[2]["content"].as_str().unwrap().contains("second"));
    assert!(output[5]["content"].as_str().unwrap().contains("second"));
}

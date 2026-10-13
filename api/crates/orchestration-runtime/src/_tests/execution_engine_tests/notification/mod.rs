use super::*;
use crate::execution_engine::NativeNotificationInvocationOutput;

struct NotificationInvoker {
    fail: bool,
    calls: Mutex<Vec<Value>>,
}

#[async_trait]
impl ProviderInvoker for NotificationInvoker {
    async fn invoke_llm(
        &self,
        _: &CompiledLlmRuntime,
        _: ProviderInvocationInput,
    ) -> Result<ProviderInvocationOutput> {
        panic!("notification graph must not invoke a model")
    }
}

#[async_trait]
impl CapabilityInvoker for NotificationInvoker {
    async fn invoke_capability_node(
        &self,
        _: &CompiledPluginRuntime,
        _: Value,
        _: Value,
    ) -> Result<CapabilityInvocationOutput> {
        panic!("native notification must not be dispatched as a legacy plugin node")
    }

    async fn invoke_notification_node(
        &self,
        node: &CompiledNode,
        inputs: &serde_json::Map<String, Value>,
    ) -> Result<NativeNotificationInvocationOutput> {
        self.calls.lock().unwrap().push(inputs["input"].clone());
        Ok(NativeNotificationInvocationOutput {
            output_payload: if self.fail {
                json!({})
            } else {
                json!({"instance_id":node.config["instance_id"],"type_code":"system","result":{"notification_ids":[Uuid::nil()],"recipient_count":1}})
            },
            error_payload: self.fail.then(|| json!({"kind":"notification_send_failed","message":"notification instance is disabled","retryable":false})),
        })
    }
}

impl_noop_code_invoker!(NotificationInvoker);

fn plan() -> CompiledPlan {
    let mut plan = super::workflow::workflow_plan();
    let node = plan.nodes.get_mut("node-transform").unwrap();
    node.node_type = "notification".into();
    node.config = json!({"instance_id":Uuid::nil()});
    node.bindings = BTreeMap::from([(
        "input".into(),
        CompiledBinding {
            i18n_text_ref: None,
            kind: "named_bindings".into(),
            raw_value: json!([
                {"name":"title","valueType":"string","value":{"kind":"templated_text","value":"Ticket {{ node-workflow-start.customer_id }}"}},
                {"name":"content","valueType":"string","value":{"kind":"selector","selector":["node-workflow-start","customer_id"]}},
                {"name":"user_ids","valueType":"array","value":{"kind":"constant","value":[Uuid::nil()]}}
            ]),
            selector_paths: vec![vec!["node-workflow-start".into(), "customer_id".into()]],
        },
    )]);
    node.outputs = vec![CompiledOutput {
        key: "result".into(),
        title: "Result".into(),
        value_type: "object".into(),
        selector: vec!["result".into()],
        json_schema: Some(
            json!({"type":"object","properties":{"recipient_count":{"type":"integer"}}}),
        ),
    }];
    let end = plan.nodes.get_mut("node-workflow-end").unwrap();
    end.bindings = BTreeMap::from([(
        "ticket_id".into(),
        CompiledBinding {
            i18n_text_ref: None,
            kind: "selector".into(),
            raw_value: json!(["node-transform", "result", "recipient_count"]),
            selector_paths: vec![vec![
                "node-transform".into(),
                "result".into(),
                "recipient_count".into(),
            ]],
        },
    )]);
    end.outputs[0].value_type = "number".into();
    plan
}

#[tokio::test]
async fn notification_resolves_bindings_and_projects_provider_result_to_downstream() {
    let invoker = NotificationInvoker {
        fail: false,
        calls: Mutex::new(vec![]),
    };
    let outcome = start_flow_debug_run(
        &plan(),
        &json!({"node-workflow-start":{"customer_id":"C-42"}}),
        &invoker,
    )
    .await
    .unwrap();
    assert!(matches!(
        outcome.stop_reason,
        ExecutionStopReason::Completed
    ));
    assert_eq!(
        *invoker.calls.lock().unwrap(),
        vec![json!({"title":"Ticket C-42","content":"C-42","user_ids":[Uuid::nil()]})]
    );
    assert_eq!(outcome.variable_pool["node-workflow-end"]["ticket_id"], 1);
    assert_eq!(outcome.node_traces[1].output_payload["type_code"], "system");
}

#[tokio::test]
async fn notification_failure_stops_downstream_and_preserves_structured_error() {
    let invoker = NotificationInvoker {
        fail: true,
        calls: Mutex::new(vec![]),
    };
    let outcome = start_flow_debug_run(
        &plan(),
        &json!({"node-workflow-start":{"customer_id":"C-42"}}),
        &invoker,
    )
    .await
    .unwrap();
    assert!(matches!(
        outcome.stop_reason,
        ExecutionStopReason::Failed(_)
    ));
    assert_eq!(invoker.calls.lock().unwrap().len(), 1);
    assert!(!outcome.variable_pool.contains_key("node-workflow-end"));
    assert_eq!(
        outcome
            .node_traces
            .last()
            .unwrap()
            .error_payload
            .as_ref()
            .unwrap()["kind"],
        "notification_send_failed"
    );
}

#[tokio::test]
async fn notification_preview_sends_once_using_explicit_upstream_values() {
    for fail in [false, true] {
        let invoker = NotificationInvoker {
            fail,
            calls: Mutex::new(vec![]),
        };
        let preview = crate::preview_executor::run_node_preview(
            &plan(),
            "node-transform",
            &json!({"node-workflow-start":{"customer_id":"C-42"}}),
            &invoker,
        )
        .await
        .unwrap();
        assert_eq!(preview.is_failed(), fail);
        assert_eq!(preview.resolved_inputs["input"]["title"], "Ticket C-42");
        assert_eq!(invoker.calls.lock().unwrap().len(), 1);
        if !fail {
            assert_eq!(preview.node_output["result"]["recipient_count"], 1);
        }
    }
}

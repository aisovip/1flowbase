use super::*;

#[test]
fn notification_compilation_requires_an_instance_uuid() {
    for config in [
        json!({}),
        json!({"instance_id":"invalid"}),
        json!({"instance_id":42}),
    ] {
        let mut document = sample_document(Uuid::nil());
        document["graph"]["nodes"][1]["type"] = json!("notification");
        document["graph"]["nodes"][1]["config"] = config;
        let error =
            FlowCompiler::compile(Uuid::nil(), "draft", &document, &compile_context()).unwrap_err();
        assert!(error.to_string().contains("instance_id"));
    }
    let mut document = sample_document(Uuid::nil());
    document["graph"]["nodes"][1]["type"] = json!("notification");
    document["graph"]["nodes"][1]["config"] = json!({"instance_id":Uuid::nil()});
    let plan = FlowCompiler::compile(Uuid::nil(), "draft", &document, &compile_context()).unwrap();
    assert_eq!(
        plan.nodes["node-llm"].config["instance_id"],
        Uuid::nil().to_string()
    );
    assert!(plan.nodes["node-llm"].plugin_runtime.is_none());
    assert!(!plan
        .compile_issues
        .iter()
        .any(|issue| issue.code == CompileIssueCode::UnsupportedNodeType));
    ensure_plan_execution_contract(&plan)
        .expect("authored notification nodes must pass the production execution contract");
}

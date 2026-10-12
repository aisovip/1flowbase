use super::*;

fn reference() -> ResourceReference {
    ResourceReference {
        v: 1,
        user_id: uuid::Uuid::from_u128(10),
        workspace_id: uuid::Uuid::from_u128(20),
        instance_id: "selected".into(),
        tool_id: "plugin.ssh.read_resource".into(),
        des_id: "current-revision".into(),
        arguments: json!({"host_id":"host","path":"/tmp/file.bin"}),
        resource_uri: "file:///tmp/file.bin".into(),
    }
}

#[test]
fn resource_reference_roundtrips_without_binary_and_denies_unknown_payload_fields() {
    let original = reference();
    let uri = original.encode().unwrap();
    assert!(uri.starts_with(PREFIX));
    let decoded = ResourceReference::decode(&uri).unwrap();
    assert_eq!(decoded.arguments, original.arguments);
    assert_eq!(decoded.resource_uri, original.resource_uri);
    let mut payload = serde_json::to_value(&original).unwrap();
    assert!(payload.get("blob").is_none());
    assert!(payload.get("headers").is_none());
    payload["secret"] = json!("not permitted");
    let unknown = format!(
        "{PREFIX}{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap())
    );
    assert!(ResourceReference::decode(&unknown).is_err());
    payload.as_object_mut().unwrap().remove("secret");
    payload["v"] = json!(2);
    let future = format!(
        "{PREFIX}{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap())
    );
    assert!(ResourceReference::decode(&future).is_err());
    for invalid in [
        "https://example.com/file",
        "file:///tmp/file",
        "flowbase-resource://other/a",
        "flowbase-resource://read/%%",
        "flowbase-resource://read/e30",
    ] {
        assert!(ResourceReference::decode(invalid).is_err());
    }
}

#[test]
fn references_follow_actual_tool_arguments_transport_budget() {
    let mut oversized = reference();
    oversized.arguments = json!({"path":"x".repeat(2 * 1024 * 1024)});
    assert!(oversized.encode().is_err());
    assert!(
        ResourceReference::decode(&format!("{PREFIX}{}", "a".repeat(2 * 1024 * 1024))).is_err()
    );
    let mut too_large_after_encoding = reference();
    too_large_after_encoding.arguments = json!({"path":"x".repeat(1_600_000)});
    assert!(McpToolArguments::from_protocol(too_large_after_encoding.arguments.clone()).is_ok());
    assert!(too_large_after_encoding.encode().is_err());
}

#[test]
fn resources_rpc_projects_through_tool_call_and_preserves_standard_result_shapes() {
    assert_eq!(list_result(), json!({"resources":[]}));
    assert_eq!(capabilities(), json!({"tools":{},"resources":{}}));
    let params = json!({"uri":"urn:test:a","_meta":{"ignored":true}});
    let (name, arguments) = super::super::protocol_call("resources/read", &params).unwrap();
    assert_eq!(name, READ_TOOL);
    assert_eq!(arguments, json!({"uri":"urn:test:a"}));
    let list_params = json!({});
    let (name, arguments) = super::super::protocol_call("resources/list", &list_params).unwrap();
    assert_eq!(name, LIST_TOOL);
    assert_eq!(arguments, json!({}));
    assert!(super::super::protocol_call("resources/read", &json!({"uri":1})).is_err());
    let result = json!({"content":[
        {"type":"resource","resource":{"uri":"urn:test:a","mimeType":"application/octet-stream","blob":"YQ=="}},
        {"type":"resource","resource":{"uri":"urn:test:b","text":"other"}},
        {"type":"resource_link","uri":"urn:test:a","name":"file"}
    ]});
    assert_eq!(
        read_result(&result, "urn:test:a", "flowbase-resource://read/reference").unwrap(),
        json!({"contents":[{"uri":"flowbase-resource://read/reference","mimeType":"application/octet-stream","blob":"YQ=="}]})
    );
    assert!(read_result(&result, "urn:test:missing", "requested").is_err());
    assert!(read_result(
        &json!({"isError":true,"content":result["content"]}),
        "urn:test:a",
        "requested"
    )
    .is_err());
}

#[test]
fn host_reference_is_a_valid_uri_and_preserves_final_binary_budget() {
    use base64::engine::general_purpose::STANDARD;
    let uri = reference().encode().unwrap();
    assert!(media::valid_uri(&uri));
    let blob = STANDARD.encode(vec![73; 524_288]);
    for result in [
        json!({"content":[{"type":"resource","resource":{"uri":uri,"mimeType":"application/octet-stream","blob":blob}}]}),
        json!({"contents":[{"uri":uri,"mimeType":"application/octet-stream","blob":blob}]}),
    ] {
        let original = result.clone();
        let (_, response) = super::super::bounded_jsonrpc_response(
            axum::http::StatusCode::OK,
            super::super::JsonRpcResponse {
                jsonrpc: "2.0",
                id: Some(json!(1)),
                result: Some(result),
                error: None,
            },
        );
        assert!(response.0.error.is_none());
        assert_eq!(response.0.result.unwrap(), original);
    }
}

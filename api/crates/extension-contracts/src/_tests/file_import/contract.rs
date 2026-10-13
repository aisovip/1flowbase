use crate::{FileImportRequest, ResourceReference};
use serde_json::json;

#[test]
fn import_request_is_identity_free_and_only_accepts_host_replay_scheme() {
    let request = json!({"resource_uri":"flowbase-resource://read/reference", "file_table_id":"00000000-0000-0000-0000-000000000001"});
    assert!(serde_json::from_value::<FileImportRequest>(request.clone())
        .unwrap()
        .validate()
        .is_ok());
    for field in [
        "actor",
        "actor_id",
        "workspace_id",
        "installation_id",
        "headers",
        "filename",
        "bytes",
    ] {
        let mut forged = request.clone();
        forged[field] = json!("forged");
        assert!(
            serde_json::from_value::<FileImportRequest>(forged).is_err(),
            "{field}"
        );
    }
    for uri in [
        "https://example.com/a",
        "file:///tmp/a",
        "flowbase-resource://other/a",
    ] {
        let mut bad = request.clone();
        bad["resource_uri"] = json!(uri);
        assert!(serde_json::from_value::<FileImportRequest>(bad)
            .unwrap()
            .validate()
            .is_err());
    }
}

#[test]
fn replay_codec_keeps_locator_identity_without_claiming_authorization() {
    let reference = ResourceReference {
        v: 1,
        user_id: uuid::Uuid::from_u128(1),
        workspace_id: uuid::Uuid::from_u128(2),
        instance_id: "selected".into(),
        tool_id: "read".into(),
        des_id: "revision".into(),
        arguments: json!({"path":"/tmp/a"}),
        resource_uri: "file:///tmp/a".into(),
    };
    let encoded = reference.encode().unwrap();
    let decoded = ResourceReference::decode(&encoded).unwrap();
    assert_eq!(decoded.user_id, reference.user_id);
    assert_eq!(decoded.arguments, reference.arguments);
    assert_eq!(decoded.resource_uri, reference.resource_uri);
    let mut recursive = reference.clone();
    recursive.resource_uri = encoded;
    assert!(recursive.encode().is_err());
    let mut unsupported = reference;
    unsupported.v = 2;
    assert!(unsupported.encode().is_err());
}

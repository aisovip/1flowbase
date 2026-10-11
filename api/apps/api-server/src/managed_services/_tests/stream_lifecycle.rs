use super::*;
#[tokio::test]
async fn socket_exit_and_mutation_cancel_the_ephemeral_call_and_release_registration() {
    let generation = streams()
        .lock()
        .unwrap()
        .generation
        .clone()
        .unwrap_or_default();
    let installation = uuid::Uuid::now_v7();
    let (lease, mut cancellation) =
        register(installation, epoch(installation), &generation).unwrap();
    assert!(!*cancellation.borrow());
    drop(lease);
    cancellation.changed().await.unwrap();
    assert!(*cancellation.borrow());
    assert!(streams()
        .lock()
        .unwrap()
        .installations
        .get(&installation)
        .unwrap()
        .sockets
        .is_empty());
    let (lease, mut cancellation) =
        register(installation, epoch(installation), &generation).unwrap();
    revoke_installation(installation);
    cancellation.changed().await.unwrap();
    assert!(*cancellation.borrow());
    assert!(register(installation, epoch(installation) - 1, &generation).is_err());
    drop(lease);
}

#[test]
fn client_frames_reject_schema_forgery_unknown_fields_and_oversize_transport() {
    let schema = jsonschema::validator_for(&json!({"type":"object","required":["data"],"properties":{"data":{"type":"string"}},"additionalProperties":false})).unwrap();
    assert!(matches!(
        parse_client_frame(r#"{"type":"input","input":{"data":"x"}}"#, &schema),
        Ok(ClientFrame::Input(_))
    ));
    assert!(matches!(
        parse_client_frame(r#"{"type":"ack","id":0}"#, &schema),
        Ok(ClientFrame::Ack(0))
    ));
    for text in [
        r#"{"type":"input","input":{"data":42}}"#,
        r#"{"type":"input","input":{"data":"x","actor":"forged"}}"#,
        r#"{"type":"ack","id":-1}"#,
        r#"{"type":"ack","id":0,"extra":true}"#,
        r#"{"type":"unknown","id":0}"#,
        "invalid",
    ] {
        assert!(parse_client_frame(text, &schema).is_err(), "{text}");
    }
    assert!(parse_client_frame(&"x".repeat(FRAME_BYTES + 1), &schema).is_err());
}

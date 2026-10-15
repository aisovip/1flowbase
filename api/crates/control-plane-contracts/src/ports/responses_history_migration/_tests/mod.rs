use super::*;
use serde_json::json;

#[test]
fn document_contract_keeps_original_json_and_rejects_arbitrary_owners() {
    let original = json!({"\0key": "proof\0bytes", "literal": "\\u0000", "nested": {"__raw_json": "user-owned"}});
    let change = ResponsesHistoryDocumentChange {
        owner: ResponsesHistoryDocumentOwner::CallbackRequest {
            callback_task_id: Uuid::nil(),
        },
        before: original.clone(),
        after: json!({"\0key": "new\0proof", "literal": "\\u0000", "nested": {"__raw_json": "user-owned"}}),
    };
    let encoded = serde_json::to_vec(&change).unwrap();
    let restored: ResponsesHistoryDocumentChange = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(restored, change);
    assert_eq!(restored.before, original);
    assert!(
        serde_json::from_value::<ResponsesHistoryDocumentOwner>(json!({
            "kind": "arbitrary_column", "table": "flow_runs", "column": "status"
        }))
        .is_err()
    );
}

#[test]
fn continuation_receipt_round_trip_keeps_cipher_and_retention_cas() {
    let before = ResponsesHistoryContinuation {
        slot_key: "claim:retained-owner".into(),
        encrypted_payload: json!({"algorithm":"aead_xchacha20poly1305_v1", "nonce":"opaque", "ciphertext":"opaque"}),
        plaintext_digest: "sha256:before".into(),
        plaintext_size_bytes: 123,
        key_version: "provider-secret-master-key:v1".into(),
        hard_expires_at: OffsetDateTime::UNIX_EPOCH,
        payload: json!({"history_proof":"old\0proof"}),
    };
    let mut after = before.clone();
    after.encrypted_payload["ciphertext"] = json!("new-cipher");
    after.plaintext_digest = "sha256:after".into();
    after.payload["history_proof"] = json!("new\0proof");
    let receipt = ResponsesHistoryContinuationReceipt { before, after };
    let restored: ResponsesHistoryContinuationReceipt =
        serde_json::from_slice(&serde_json::to_vec(&receipt).unwrap()).unwrap();
    assert_eq!(restored, receipt);
    assert_eq!(
        restored.before.hard_expires_at,
        restored.after.hard_expires_at
    );
    assert_eq!(restored.before.slot_key, restored.after.slot_key);
}

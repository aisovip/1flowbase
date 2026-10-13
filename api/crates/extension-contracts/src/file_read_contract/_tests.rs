use crate::{FileReadRequest, MultiplexEnvelope, MultiplexHostService, MultiplexWorkerMessage};
use serde_json::json;
#[test]
fn read_selectors_and_tickets_never_accept_worker_identity_or_storage_path() {
    let valid = json!({"operation":"open","file_table_id":"00000000-0000-0000-0000-000000000001","record_id":"00000000-0000-0000-0000-000000000002"});
    serde_json::from_value::<FileReadRequest>(valid.clone())
        .unwrap()
        .validate()
        .unwrap();
    for key in ["actor", "workspace_id", "storage_id", "path"] {
        let mut forged = valid.clone();
        forged[key] = json!("forged");
        assert!(serde_json::from_value::<FileReadRequest>(forged).is_err());
    }
    assert!(serde_json::from_value::<FileReadRequest>(
        json!({"operation":"open","file_table_id":"invalid","record_id":"invalid"})
    )
    .unwrap()
    .validate()
    .is_err());
    let callback = MultiplexEnvelope::new(MultiplexWorkerMessage::Callback {
        call_id: "1".into(),
        callback_id: "1".into(),
        service: MultiplexHostService::FileReadV1,
        request: valid,
    });
    assert_eq!(
        serde_json::to_value(callback).unwrap()["service"],
        "file_read_v1"
    );
}

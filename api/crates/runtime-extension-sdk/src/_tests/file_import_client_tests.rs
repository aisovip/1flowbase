use crate::{serve_io, FileImportClient, FileImportClientError};
use extension_contracts::{
    MultiplexEnvelope, MultiplexHostMessage, MultiplexHostService, MultiplexWorkerMessage,
};
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::test]
async fn file_import_client_preserves_typed_results_and_failures_without_identity() {
    tokio::task::LocalSet::new().run_until(async {
        let (host, worker) = tokio::io::duplex(8192);
        let (worker_read, worker_write) = tokio::io::split(worker);
        let runner = tokio::task::spawn_local(serve_io(worker_read, worker_write, |_, emitter| async move {
            let client = FileImportClient::new(emitter);
            let table = "00000000-0000-0000-0000-000000000001".to_owned();
            let response = client.import_resource("flowbase-resource://read/a".into(), table.clone()).await.unwrap();
            assert_eq!(response.record["filename"], "file.bin");
            assert_eq!(response.record["size"], 700000);
            assert!(matches!(client.import_resource("flowbase-resource://read/b".into(), table).await, Err(FileImportClientError::Host(error)) if error.code == "file_import_target_permission_denied"));
            json!({"ok":true})
        }));
        let (host_read, mut host_write) = tokio::io::split(host);
        let mut reader = BufReader::new(host_read);
        let mut line = serde_json::to_vec(&MultiplexEnvelope::new(MultiplexHostMessage::Call { call_id: "admitted".into(), request: json!({}) })).unwrap(); line.push(b'\n');
        host_write.write_all(&line).await.unwrap();
        for index in 0..2 {
            let mut line = String::new(); reader.read_line(&mut line).await.unwrap();
            let envelope: MultiplexEnvelope<MultiplexWorkerMessage> = serde_json::from_str(&line).unwrap();
            let MultiplexWorkerMessage::Callback { call_id, callback_id, service, request } = envelope.message else { panic!("callback expected") };
            assert_eq!(service, MultiplexHostService::FileImportV1);
            assert_eq!(call_id, "admitted"); assert_eq!(callback_id, (index + 1).to_string());
            assert_eq!(request.as_object().unwrap().len(), 2);
            assert!(request.get("actor").is_none()); assert!(request.get("workspace_id").is_none());
            let response = if index == 0 { json!({"result":{"file_table_id":request["file_table_id"],"storage_id":"storage","record":{"filename":"file.bin","size":700000}}}) } else { json!({"error":{"code":"file_import_target_permission_denied"}}) };
            let mut line = serde_json::to_vec(&MultiplexEnvelope::new(MultiplexHostMessage::CallbackResult { call_id, callback_id, response })).unwrap(); line.push(b'\n'); host_write.write_all(&line).await.unwrap();
        }
        let mut line = String::new(); reader.read_line(&mut line).await.unwrap();
        let envelope: MultiplexEnvelope<MultiplexWorkerMessage> = serde_json::from_str(&line).unwrap();
        assert!(matches!(envelope.message, MultiplexWorkerMessage::Response { .. }));
        host_write.shutdown().await.unwrap(); runner.await.unwrap().unwrap();
    }).await;
}

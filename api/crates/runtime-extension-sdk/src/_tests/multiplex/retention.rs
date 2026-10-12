use crate::{
    serve_io, MultiplexEnvelope, MultiplexError, MultiplexHostMessage, MultiplexResourceLease,
    MultiplexWorkerMessage,
};
use extension_contracts::RUNTIME_WORKER_RETENTION_V1;
use serde_json::json;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn line(message: MultiplexHostMessage) -> String {
    format!(
        "{}\n",
        serde_json::to_string(&MultiplexEnvelope::new(message)).unwrap()
    )
}
async fn read(reader: &mut (impl tokio::io::AsyncBufRead + Unpin)) -> MultiplexWorkerMessage {
    let mut line = String::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        reader.read_line(&mut line),
    )
    .await
    .unwrap()
    .unwrap();
    serde_json::from_str::<MultiplexEnvelope<MultiplexWorkerMessage>>(&line)
        .unwrap()
        .message
}
#[tokio::test(flavor = "current_thread")]
async fn resource_acquisitions_precede_response_and_background_drops_coalesce() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (host, worker) = tokio::io::duplex(4096);
            let (worker_read, worker_write) = tokio::io::split(worker);
            let (host_read, mut host_write) = tokio::io::split(host);
            let leases: Arc<Mutex<Vec<MultiplexResourceLease>>> = Arc::default();
            let storage = leases.clone();
            let runner =
                tokio::task::spawn_local(serve_io(worker_read, worker_write, move |_, emitter| {
                    let storage = storage.clone();
                    async move {
                        for _ in 0..16 {
                            let lease = emitter.retain_resource().await.unwrap();
                            storage.lock().unwrap().push(lease);
                        }
                        json!("opened")
                    }
                }));
            host_write
                .write_all(
                    line(MultiplexHostMessage::WorkerCapabilities {
                        capabilities: vec![RUNTIME_WORKER_RETENTION_V1.into()],
                    })
                    .as_bytes(),
                )
                .await
                .unwrap();
            host_write
                .write_all(
                    line(MultiplexHostMessage::Call {
                        call_id: "1".into(),
                        request: json!(null),
                    })
                    .as_bytes(),
                )
                .await
                .unwrap();
            let mut reader = BufReader::new(host_read);
            for expected in 1..=16 {
                assert_eq!(
                    read(&mut reader).await,
                    MultiplexWorkerMessage::ResourceRetention {
                        sequence: expected,
                        retained_resources: expected,
                    }
                );
            }
            assert_eq!(
                read(&mut reader).await,
                MultiplexWorkerMessage::Response {
                    call_id: "1".into(),
                    response: json!("opened"),
                }
            );
            // No call is active; Drop works off the SDK executor and queues one latest snapshot.
            std::thread::spawn(move || leases.lock().unwrap().clear())
                .join()
                .unwrap();
            assert_eq!(
                read(&mut reader).await,
                MultiplexWorkerMessage::ResourceRetention {
                    sequence: 32,
                    retained_resources: 0,
                }
            );
            host_write.shutdown().await.unwrap();
            assert!(runner.await.unwrap().is_ok());
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn resource_retention_fails_explicitly_without_host_capability_advertisement() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (host, worker) = tokio::io::duplex(4096);
            let (worker_read, worker_write) = tokio::io::split(worker);
            let (host_read, mut host_write) = tokio::io::split(host);
            let runner = tokio::task::spawn_local(serve_io(
                worker_read,
                worker_write,
                |_, emitter| async move {
                    json!(matches!(
                        emitter.retain_resource().await,
                        Err(MultiplexError::Protocol)
                    ))
                },
            ));
            host_write
                .write_all(
                    line(MultiplexHostMessage::Call {
                        call_id: "1".into(),
                        request: json!(null),
                    })
                    .as_bytes(),
                )
                .await
                .unwrap();
            let mut reader = BufReader::new(host_read);
            assert_eq!(
                read(&mut reader).await,
                MultiplexWorkerMessage::Response {
                    call_id: "1".into(),
                    response: json!(true)
                }
            );
            host_write.shutdown().await.unwrap();
            assert!(runner.await.unwrap().is_ok());
        })
        .await;
}

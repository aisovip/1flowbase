use crate::{serve_io, FileReadClient};
use base64::{engine::general_purpose::STANDARD, Engine};
use extension_contracts::{
    MultiplexEnvelope, MultiplexHostMessage, MultiplexHostService, MultiplexWorkerMessage,
    FILE_READ_CHUNK_BYTES,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

async fn transfer(length: u64, fault: &str) {
    let fault = fault.to_owned();
    tokio::task::LocalSet::new().run_until(async move {
        let success = fault.is_empty();
        let (host, worker) = tokio::io::duplex(256 * 1024);
        let (worker_read, worker_write) = tokio::io::split(worker);
        let runner = tokio::task::spawn_local(serve_io(worker_read, worker_write, move |_, emitter| async move {
            let mut read = FileReadClient::new(emitter).open("00000000-0000-0000-0000-000000000001".into(), "00000000-0000-0000-0000-000000000002".into()).await.unwrap();
            assert_eq!(read.metadata().content_length, length);
            let result = read.copy_to(&mut tokio::io::sink()).await;
            assert_eq!(result.is_ok(), success);
            if let Ok(report) = result { assert_eq!(report.bytes, length); }
            json!({"ok":true})
        }));
        let (host_read, mut host_write) = tokio::io::split(host);
        let mut reader = BufReader::new(host_read);
        let mut line = serde_json::to_vec(&MultiplexEnvelope::new(MultiplexHostMessage::Call { call_id: "1".into(), request: json!({}) })).unwrap(); line.push(b'\n'); host_write.write_all(&line).await.unwrap();
        let ticket = "00000000-0000-0000-0000-000000000003";
        let mut hash = Sha256::new();
        let mut closed = false;
        let mut frames = 0;
        loop {
            let mut line = String::new(); reader.read_line(&mut line).await.unwrap();
            let envelope: MultiplexEnvelope<MultiplexWorkerMessage> = serde_json::from_str(&line).unwrap();
            let MultiplexWorkerMessage::Callback { call_id, callback_id, service, request } = envelope.message else { assert!(closed); break; };
            assert_eq!(service, MultiplexHostService::FileReadV1);
            assert!(request.get("actor").is_none()); assert!(request.get("storage_id").is_none());
            let result = match request["operation"].as_str().unwrap() {
                "open" => json!({"operation":"open","ticket":ticket,"metadata":{"filename":"file.bin","content_type":"application/octet-stream","content_length":length}}),
                "close" => { closed = true; json!({"operation":"close","ticket":ticket}) },
                "chunk" => {
                    frames += 1;
                    let offset = request["offset"].as_u64().unwrap();
                    let data = vec![42u8; (length-offset).min(FILE_READ_CHUNK_BYTES as u64) as usize];
                    hash.update(&data);
                    let eof = offset + data.len() as u64 == length;
                    let digest = eof.then(|| format!("{:x}",base16ct::HexDisplay(&hash.clone()).finalize()));
                    let mut response = json!({"operation":"chunk","ticket":ticket,"offset":offset,"data_base64":STANDARD.encode(data),"eof":eof,"sha256":digest});
                    match fault.as_str() {
                        "offset" => response["offset"] = json!(offset + 1),
                        "base64" => response["data_base64"] = json!("!invalid"),
                        "oversize" => response["data_base64"] = json!(STANDARD.encode(vec![0; FILE_READ_CHUNK_BYTES+1])),
                        "truncated" => { response["data_base64"]=json!(""); response["eof"]=json!(true); },
                        "hash" if eof => response["sha256"] = json!("wrong"),
                        _ => {},
                    }
                    response
                },
                _ => panic!("unexpected operation"),
            };
            let mut line = serde_json::to_vec(&MultiplexEnvelope::new(MultiplexHostMessage::CallbackResult { call_id, callback_id, response:json!({"result":result}) })).unwrap(); line.push(b'\n'); host_write.write_all(&line).await.unwrap();
        }
        if length > 32*1024*1024 && success { assert!(frames > 512); }
        host_write.shutdown().await.unwrap(); runner.await.unwrap().unwrap();
    }).await;
}
#[tokio::test]
async fn verified_pump_streams_more_than_worker_business_budget_and_empty_files() {
    transfer(33 * 1024 * 1024 + 3, "").await;
    transfer(0, "").await;
}
#[tokio::test]
async fn verified_pump_rejects_controlled_corrupt_frames_and_always_closes() {
    for fault in ["offset", "base64", "oversize", "truncated", "hash"] {
        transfer(10, fault).await;
    }
}

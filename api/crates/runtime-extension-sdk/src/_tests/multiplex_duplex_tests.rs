use crate::{serve_duplex_io, MultiplexError};
use extension_contracts::{MultiplexEnvelope, MultiplexHostMessage, MultiplexWorkerMessage};
use serde_json::{json, Value};
use std::{cell::Cell, rc::Rc, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

async fn send(writer: &mut (impl tokio::io::AsyncWrite + Unpin), message: MultiplexHostMessage) {
    let line = format!(
        "{}\n",
        serde_json::to_string(&MultiplexEnvelope::new(message)).unwrap()
    );
    writer.write_all(line.as_bytes()).await.unwrap();
}
async fn read(reader: &mut (impl tokio::io::AsyncBufRead + Unpin)) -> MultiplexWorkerMessage {
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut line))
        .await
        .unwrap()
        .unwrap();
    serde_json::from_str::<MultiplexEnvelope<MultiplexWorkerMessage>>(&line)
        .unwrap()
        .message
}

#[tokio::test(flavor = "current_thread")]
async fn duplex_pipelines_events_before_ack_and_input_remains_live() {
    tokio::task::LocalSet::new().run_until(async {
        let (host, worker) = tokio::io::duplex(65536);
        let (worker_read, worker_write) = tokio::io::split(worker);
        let (host_read, mut writer) = tokio::io::split(host);
        let cleaned = Rc::new(Cell::new(false));
        let cleanup = cleaned.clone();
        let runner = tokio::task::spawn_local(serve_duplex_io(worker_read, worker_write, move |request, emitter, mut input| {
            let cleanup = cleanup.clone();
            async move {
                if request == json!("neighbor") { return json!("neighbor-done"); }
                emitter.event(json!({"output":1})).await.unwrap();
                emitter.event(json!({"output":2})).await.unwrap();
                assert_eq!(input.recv().await, Some(json!("ctrl-c")));
                emitter.event(json!({"output":3})).await.unwrap();
                assert!(input.recv().await.is_none());
                // Cancellation closes input and permits awaited resource cleanup before terminal.
                tokio::task::yield_now().await;
                cleanup.set(true);
                json!("closed")
            }
        }));
        let mut reader = BufReader::new(host_read);
        send(&mut writer, MultiplexHostMessage::Call { call_id:"1".into(), request:json!("pty") }).await;
        for expected in 1..=2 {
            match read(&mut reader).await {
                MultiplexWorkerMessage::Event { event, await_ack:true, event_id:Some(id), .. } => {
                    assert_eq!(event["output"], expected);
                    assert_eq!(id, expected.to_string());
                }
                other => panic!("expected pipelined event, got {other:?}"),
            }
        }
        // No event ACK has been sent. Input and neighboring unary calls still progress.
        send(&mut writer, MultiplexHostMessage::Input { call_id:"1".into(), input:json!("ctrl-c") }).await;
        match read(&mut reader).await { MultiplexWorkerMessage::Event { event, .. } => assert_eq!(event["output"],3), other => panic!("{other:?}") }
        send(&mut writer, MultiplexHostMessage::Call { call_id:"2".into(), request:json!("neighbor") }).await;
        assert!(matches!(read(&mut reader).await, MultiplexWorkerMessage::Response { call_id, response } if call_id == "2" && response == json!("neighbor-done")));
        send(&mut writer, MultiplexHostMessage::Cancel { call_id:"1".into() }).await;
        assert!(matches!(read(&mut reader).await, MultiplexWorkerMessage::Cancelled { call_id } if call_id == "1"));
        assert!(cleaned.get());
        writer.shutdown().await.unwrap();
        runner.await.unwrap().unwrap();
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn duplex_terminal_waits_for_output_consumption_and_rejects_unknown_ack() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (host, worker) = tokio::io::duplex(4096);
            let (worker_read, worker_write) = tokio::io::split(worker);
            let (host_read, mut writer) = tokio::io::split(host);
            let runner = tokio::task::spawn_local(serve_duplex_io(
                worker_read,
                worker_write,
                |_, emitter, _| async move {
                    emitter.event(json!("final-output")).await.unwrap();
                    Value::Null
                },
            ));
            let mut reader = BufReader::new(host_read);
            send(
                &mut writer,
                MultiplexHostMessage::Call {
                    call_id: "1".into(),
                    request: Value::Null,
                },
            )
            .await;
            assert!(matches!(
                read(&mut reader).await,
                MultiplexWorkerMessage::Event { .. }
            ));
            let mut line = String::new();
            assert!(
                tokio::time::timeout(Duration::from_millis(20), reader.read_line(&mut line))
                    .await
                    .is_err()
            );
            send(
                &mut writer,
                MultiplexHostMessage::EventAck {
                    call_id: "1".into(),
                    event_id: "1".into(),
                },
            )
            .await;
            assert!(matches!(
                read(&mut reader).await,
                MultiplexWorkerMessage::Response { .. }
            ));
            send(
                &mut writer,
                MultiplexHostMessage::EventAck {
                    call_id: "999".into(),
                    event_id: "1".into(),
                },
            )
            .await;
            assert!(matches!(
                runner.await.unwrap(),
                Err(MultiplexError::Correlation)
            ));
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn duplex_rejects_unknown_input_without_creating_a_call() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (host, worker) = tokio::io::duplex(4096);
            let (worker_read, worker_write) = tokio::io::split(worker);
            let (_, mut writer) = tokio::io::split(host);
            let runner = tokio::task::spawn_local(serve_duplex_io(
                worker_read,
                worker_write,
                |_, _, _| async { Value::Null },
            ));
            send(
                &mut writer,
                MultiplexHostMessage::Input {
                    call_id: "1".into(),
                    input: Value::Null,
                },
            )
            .await;
            assert!(matches!(
                runner.await.unwrap(),
                Err(MultiplexError::Correlation)
            ));
        })
        .await;
}

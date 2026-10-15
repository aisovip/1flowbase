use crate::*;
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
#[tokio::test]
async fn exchange_host_services_clients_share_demux_and_preserve_status_receipts() {
    tokio::task::LocalSet::new().run_until(async {
        let (host, worker) = tokio::io::duplex(8192);
        let (worker_read, worker_write) = tokio::io::split(worker);
        let runner = tokio::task::spawn_local(serve_io(worker_read, worker_write, |_, emitter| async move {
            let logs = ApplicationLogsClient::new(emitter.clone());
            let http = HttpEgressClient::new(emitter);
            let response = logs.execute(ApplicationLogsRequest::ListApplications {application_types:vec!["AgentLogs".into()]}).await.unwrap();
            assert!(matches!(response,ApplicationLogsResponse::Applications {items} if items.len()==1));
            let response = http.execute(HttpEgressRequest {method:"GET".into(),url:"https://example.com/".into(),headers:Default::default(),body_base64:None}).await.unwrap();
            assert_eq!(response.status,429); assert_eq!(response.body_base64,"cmV0cnk=");
            let error = logs.execute(ApplicationLogsRequest::ImportOtlp {application_id:"target".into(),otlp:json!({})}).await.unwrap_err();
            assert!(matches!(error,ApplicationLogsClientError::Host(HostServiceError {status:Some(409),details:Some(details),..}) if details["accepted_events"]==3));
            json!({"ok":true})
        }));
        let (host_read,mut host_write)=tokio::io::split(host);let mut reader=BufReader::new(host_read);
        let mut frame=serde_json::to_vec(&MultiplexEnvelope::new(MultiplexHostMessage::Call {call_id:"1".into(),request:json!({})})).unwrap(); frame.push(b'\n'); host_write.write_all(&frame).await.unwrap();
        for index in 0..3 {
            let mut line=String::new(); reader.read_line(&mut line).await.unwrap();
            let envelope:MultiplexEnvelope<MultiplexWorkerMessage>=serde_json::from_str(&line).unwrap();
            let MultiplexWorkerMessage::Callback {call_id,callback_id,service,request}=envelope.message else {panic!("callback expected")};
            assert_eq!(callback_id,(index+1).to_string());assert!(request.get("actor").is_none());assert!(request.get("workspace_id").is_none());
            assert_eq!(service,if index==1 {MultiplexHostService::HttpEgressV1}else{MultiplexHostService::ApplicationLogsV1});
            let response=match index {
                0=>json!({"result":{"operation":"applications","items":[{"application_id":"a","name":"logs","application_type":"AgentLogs"}]}}),
                1=>json!({"result":{"status":429,"headers":{},"body_base64":"cmV0cnk="}}),
                _=>json!({"error":{"code":"agent_logs_partial_import","status":409,"details":{"accepted_events":3}}}),
            };
            let mut frame=serde_json::to_vec(&MultiplexEnvelope::new(MultiplexHostMessage::CallbackResult {call_id,callback_id,response})).unwrap(); frame.push(b'\n');host_write.write_all(&frame).await.unwrap();
        }
        let mut line=String::new();reader.read_line(&mut line).await.unwrap();
        assert!(matches!(serde_json::from_str::<MultiplexEnvelope<MultiplexWorkerMessage>>(&line).unwrap().message,MultiplexWorkerMessage::Response {..}));
        host_write.shutdown().await.unwrap();runner.await.unwrap().unwrap();
    }).await;
}

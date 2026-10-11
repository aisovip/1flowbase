use crate::managed_worker::{LoadedManagedBinding, ManagedWorkers};
use extension_contracts::*;
use extension_package_runtime::{PluginExecutionMode, PluginRuntimeLimits};
use runtime_core::runtime_backend::{RuntimeExecutionPrincipal, RuntimeManagedCapabilityRequest};
use std::{path::PathBuf, sync::Arc};

#[derive(Default)]
struct DataPort(tokio::sync::Mutex<Vec<PluginDataBinding>>);
impl PluginDataPort for DataPort {
    fn execute<'a>(
        &'a self,
        binding: &'a PluginDataBinding,
        _: &'a PluginDataRequest,
    ) -> PluginDataFuture<'a> {
        Box::pin(async move {
            self.0.lock().await.push(binding.clone());
            Ok(PluginDataResponse {
                results: vec![PluginDataOperationResult::Count { count: 3 }],
                replayed: false,
            })
        })
    }
}

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
#[cfg(unix)]
async fn managed_capability_multiplex_binds_data_and_rejects_stale_or_foreign_scope() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!(
        "managed-capability-{}-{}",
        std::process::id(),
        time::OffsetDateTime::now_utc().unix_timestamp_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let _cleanup = Fixture(root.clone());
    let executable = root.join("worker.py");
    std::fs::write(&executable, r#"#!/usr/bin/env python3
import json, sys
call=json.loads(sys.stdin.readline())
assert call['request']['method']=='execute'
assert call['request']['input']['handler']=='list'
print(json.dumps({'protocol':'stdio_json_multiplex_v1','kind':'callback','call_id':call['call_id'],'callback_id':'1','service':'plugin_data_v1','request':{'operations':[{'operation':'count','target':{'kind':'owned_collection','collection_code':'hosts'},'filters':[]}]}}),flush=True)
reply=json.loads(sys.stdin.readline())
print(json.dumps({'protocol':'stdio_json_multiplex_v1','kind':'response','call_id':call['call_id'],'response':{'ok':True,'result':reply['response']}}),flush=True)
sys.stdin.read()
"#).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let identity = ManagedExecutionIdentity::new(
        ManagedInstallationId::new("installation").unwrap(),
        ManagedWorkspaceId::new("system-scope").unwrap(),
        ContributionId::new("service.list").unwrap(),
        ManagedArtifactFingerprint::from_bytes(b"manifest"),
        ManagedBindingFingerprint::from_bytes(b"binding"),
    );
    let mut workers = ManagedWorkers::default();
    let handle = workers
        .mount(
            identity.clone(),
            LoadedManagedBinding {
                plugin_id: "fixture.service@1.0.0".into(),
                publisher_namespace: "fixture".into(),
                plugin_code: "fixture.service".into(),
                plugin_version: "1.0.0".into(),
                protocol: STDIO_JSON_MULTIPLEX_V1.into(),
                runtime_executable: executable.clone(),
                executable_fingerprint: ManagedArtifactFingerprint::from_bytes(
                    &std::fs::read(&executable).unwrap(),
                ),
                execution_mode: PluginExecutionMode::ProcessPerCall,
                limits: PluginRuntimeLimits {
                    timeout_ms: Some(5000),
                    ..Default::default()
                },
                handler: "list".into(),
                worker_idle_timeout_ms: None,
                interface_protocol: None,
                contribution: ContributionDescriptor {
                    contribution_id: identity.contribution_id().clone(),
                    contributor_module_id: ModuleId::new("fixture.service").unwrap(),
                    point_id: ExtensionPointId::new("1flowbase.managed-service.operation").unwrap(),
                    contract_version: ContractVersion::new("1").unwrap(),
                    required_permissions: [PermissionCode::new("plugin_data.owned.read").unwrap()]
                        .into(),
                    mode: ContributionMode::Append,
                    ordering: Default::default(),
                },
            },
        )
        .unwrap();
    let request = |scope: &str| RuntimeManagedCapabilityRequest {
        handle: handle.clone(),
        principal: RuntimeExecutionPrincipal {
            workspace_id: scope.into(),
            actor_id: Some("actor".into()),
            deadline_unix_ms: (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000)
                as i64
                + 5000,
        },
        config_payload: serde_json::json!({}),
        input_payload: serde_json::json!({}),
        duplex: None,
    };
    let port = Arc::new(DataPort::default());
    assert!(workers
        .execute_with_services(request("foreign"), port.clone(), None)
        .is_err());
    let output = workers
        .execute_with_services(request("system-scope"), port.clone(), None)
        .unwrap()
        .await
        .unwrap();
    let calls = port.0.lock().await;
    assert_eq!(calls.len(), 1, "callback must reach the real bound service");
    assert_eq!(calls[0].workspace_id, "system-scope");
    assert_eq!(calls[0].publisher_namespace, "fixture");
    assert_eq!(calls[0].provider_instance_id, "installation");
    assert!(output.to_string().contains('3'));
    drop(calls);
    workers.unmount(&handle).unwrap();
    assert!(workers
        .execute_with_services(request("system-scope"), port, None)
        .is_err());
}

#[cfg(unix)]
fn service_fixture() -> (Fixture, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!(
        "actor-service-{}-{}",
        std::process::id(),
        time::OffsetDateTime::now_utc().unix_timestamp_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let executable = root.join("worker.py");
    std::fs::write(&executable, r#"#!/usr/bin/env python3
import json, os, sys
value = None
for line in sys.stdin:
    frame = json.loads(line)
    if frame['kind'] == 'cancel':
        print(json.dumps({'protocol':'stdio_json_multiplex_v1','kind':'cancelled','call_id':frame['call_id']}),flush=True)
        continue
    body = frame['request']['input']['input_payload']
    if body.get('wait_file'):
        open(body['wait_file'],'w').close()
        continue
    if 'set' in body: value = body['set']
    print(json.dumps({'protocol':'stdio_json_multiplex_v1','kind':'response','call_id':frame['call_id'],'response':{'ok':True,'result':{'pid':os.getpid(),'value':value}}}),flush=True)
"#).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    (Fixture(root), executable)
}
#[cfg(unix)]
fn mount_service(
    workers: &mut ManagedWorkers,
    executable: &std::path::Path,
    contribution: &str,
    idle: Option<u64>,
) -> ManagedExecutionHandle {
    let identity = ManagedExecutionIdentity::new(
        ManagedInstallationId::new("installation").unwrap(),
        ManagedWorkspaceId::new("system-scope").unwrap(),
        ContributionId::new(contribution).unwrap(),
        ManagedArtifactFingerprint::from_bytes(b"manifest"),
        ManagedBindingFingerprint::from_bytes(contribution.as_bytes()),
    );
    workers
        .mount(
            identity.clone(),
            LoadedManagedBinding {
                plugin_id: "fixture.service@1.0.0".into(),
                publisher_namespace: "fixture".into(),
                plugin_code: "fixture.service".into(),
                plugin_version: "1.0.0".into(),
                protocol: STDIO_JSON_MULTIPLEX_V1.into(),
                runtime_executable: executable.into(),
                executable_fingerprint: ManagedArtifactFingerprint::from_bytes(
                    &std::fs::read(executable).unwrap(),
                ),
                execution_mode: PluginExecutionMode::ProcessPerCall,
                limits: PluginRuntimeLimits {
                    timeout_ms: Some(5000),
                    ..Default::default()
                },
                handler: contribution.into(),
                worker_idle_timeout_ms: idle,
                interface_protocol: None,
                contribution: ContributionDescriptor {
                    contribution_id: identity.contribution_id().clone(),
                    contributor_module_id: ModuleId::new("fixture.service").unwrap(),
                    point_id: ExtensionPointId::new("1flowbase.managed-service.operation").unwrap(),
                    contract_version: ContractVersion::new("1").unwrap(),
                    required_permissions: Default::default(),
                    mode: ContributionMode::Append,
                    ordering: Default::default(),
                },
            },
        )
        .unwrap()
}
#[cfg(unix)]
fn service_request(
    handle: &ManagedExecutionHandle,
    actor: Option<&str>,
    body: serde_json::Value,
) -> RuntimeManagedCapabilityRequest {
    RuntimeManagedCapabilityRequest {
        handle: handle.clone(),
        principal: RuntimeExecutionPrincipal {
            workspace_id: "system-scope".into(),
            actor_id: actor.map(str::to_owned),
            deadline_unix_ms: (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000)
                as i64
                + 5000,
        },
        config_payload: serde_json::json!({}),
        input_payload: body,
        duplex: None,
    }
}
#[cfg(unix)]
async fn reap_service(workers: &mut ManagedWorkers, handle: &ManagedExecutionHandle) {
    let scope = workers.unmount(handle).unwrap();
    if let Some(pool) = workers.service_pool_for_handle(handle).unwrap() {
        pool.retire_all().await.unwrap();
    }
    scope.dispose().await.unwrap();
    workers.finish_unmount(handle);
}
#[cfg(unix)]
fn process_exists(pid: i64) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[tokio::test]
#[cfg(unix)]
async fn actor_scope_reuses_across_contributions_and_isolates_actors_then_reaps() {
    let (_fixture, executable) = service_fixture();
    let mut workers = ManagedWorkers::default();
    let open = mount_service(&mut workers, &executable, "service.open", Some(1800000));
    let pull = mount_service(&mut workers, &executable, "service.pull", Some(1800000));
    assert!(workers
        .execute(service_request(&open, None, serde_json::json!({})))
        .is_err());
    let first = workers
        .execute(service_request(
            &open,
            Some("alice"),
            serde_json::json!({"set":"shell"}),
        ))
        .unwrap()
        .await
        .unwrap();
    let next = workers
        .execute(service_request(&pull, Some("alice"), serde_json::json!({})))
        .unwrap()
        .await
        .unwrap();
    assert_eq!(
        first, next,
        "different authorized contributions use one actor worker"
    );
    let other = workers
        .execute(service_request(&pull, Some("bob"), serde_json::json!({})))
        .unwrap()
        .await
        .unwrap();
    assert_ne!(other["pid"], first["pid"]);
    assert!(
        other["value"].is_null(),
        "a session identifier cannot cross actor process ownership"
    );
    let admitted = workers
        .execute(service_request(&pull, Some("alice"), serde_json::json!({})))
        .unwrap();
    // Unmount one shared contribution fences unpolled calls and reaps all owner processes.
    let scope = workers.unmount(&open).unwrap();
    workers
        .service_pool_for_handle(&open)
        .unwrap()
        .unwrap()
        .retire_all()
        .await
        .unwrap();
    assert!(admitted.await.is_err());
    scope.dispose().await.unwrap();
    workers.finish_unmount(&open);
    assert!(!process_exists(first["pid"].as_i64().unwrap()));
    assert!(!process_exists(other["pid"].as_i64().unwrap()));
    assert!(workers
        .execute(service_request(&open, Some("alice"), serde_json::json!({})))
        .is_err());
    let remounted = mount_service(&mut workers, &executable, "service.open", Some(1800000));
    let fresh = workers
        .execute(service_request(
            &remounted,
            Some("alice"),
            serde_json::json!({}),
        ))
        .unwrap()
        .await
        .unwrap();
    assert!(
        fresh["value"].is_null(),
        "remount must not resurrect the disposed process state"
    );
    reap_service(&mut workers, &remounted).await;
    reap_service(&mut workers, &pull).await;
}

#[tokio::test]
#[cfg(unix)]
async fn actor_scope_cancellation_does_not_kill_other_calls_or_idle_active_worker() {
    let (fixture, executable) = service_fixture();
    let mut workers = ManagedWorkers::default();
    let handle = mount_service(&mut workers, &executable, "service.open", Some(80));
    let first = workers
        .execute(service_request(
            &handle,
            Some("alice"),
            serde_json::json!({"set":"shell"}),
        ))
        .unwrap()
        .await
        .unwrap();
    let ready = fixture.0.join("waiting");
    let waiting = workers
        .execute(service_request(
            &handle,
            Some("alice"),
            serde_json::json!({"wait_file":ready}),
        ))
        .unwrap();
    let task = tokio::spawn(waiting);
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while !ready.exists() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(160)).await;
    let concurrent = workers
        .execute(service_request(
            &handle,
            Some("alice"),
            serde_json::json!({}),
        ))
        .unwrap()
        .await
        .unwrap();
    assert_eq!(first, concurrent, "idle timer cannot retire an active call");
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let after = workers
        .execute(service_request(
            &handle,
            Some("alice"),
            serde_json::json!({}),
        ))
        .unwrap()
        .await
        .unwrap();
    assert_eq!(first, after, "Cancel applies only to its multiplex call");
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while process_exists(first["pid"].as_i64().unwrap()) {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    reap_service(&mut workers, &handle).await;
}

#[tokio::test]
#[cfg(unix)]
async fn omitted_lifetime_keeps_per_call_processes_and_state_isolation() {
    let (_fixture, executable) = service_fixture();
    let mut workers = ManagedWorkers::default();
    let handle = mount_service(&mut workers, &executable, "service.open", None);
    let first = workers
        .execute(service_request(
            &handle,
            Some("alice"),
            serde_json::json!({"set":"shell"}),
        ))
        .unwrap()
        .await
        .unwrap();
    assert!(!process_exists(first["pid"].as_i64().unwrap()));
    let second = workers
        .execute(service_request(
            &handle,
            Some("alice"),
            serde_json::json!({}),
        ))
        .unwrap()
        .await
        .unwrap();
    assert!(second["value"].is_null());
    assert_ne!(first["pid"], second["pid"]);
    reap_service(&mut workers, &handle).await;
}

#[tokio::test]
#[cfg(unix)]
async fn actor_scope_unmount_reaps_an_active_worker_before_scope_disposal() {
    let (fixture, executable) = service_fixture();
    let mut workers = ManagedWorkers::default();
    let handle = mount_service(&mut workers, &executable, "service.open", Some(1800000));
    let first = workers
        .execute(service_request(
            &handle,
            Some("alice"),
            serde_json::json!({}),
        ))
        .unwrap()
        .await
        .unwrap();
    let ready = fixture.0.join("waiting-unmount");
    let task = tokio::spawn(
        workers
            .execute(service_request(
                &handle,
                Some("alice"),
                serde_json::json!({"wait_file":ready}),
            ))
            .unwrap(),
    );
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while !ready.exists() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        reap_service(&mut workers, &handle),
    )
    .await
    .unwrap();
    assert!(
        !process_exists(first["pid"].as_i64().unwrap()),
        "dispose cannot acknowledge an unreaped process"
    );
    assert!(task.await.unwrap().is_err());
}

#[cfg(unix)]
fn duplex_fixture() -> (Fixture, PathBuf) {
    let (fixture, executable) = service_fixture();
    std::fs::write(&executable, r#"#!/usr/bin/env python3
import json, os, sys
counts={}
def send(kind, cid, **fields):
    print(json.dumps(dict(protocol='stdio_json_multiplex_v1',kind=kind,call_id=cid,**fields)),flush=True)
def event(cid, value):
    counts[cid]=counts.get(cid,0)+1
    send('event',cid,event={'pty_output':value},await_ack=True,event_id=str(counts[cid]))
for line in sys.stdin:
    f=json.loads(line); cid=f['call_id']
    if f['kind']=='call':
        body=f['request']['input']['input_payload']
        if body.get('duplex'):
            event(cid,'first'); event(cid,'second')
        else: send('response',cid,response={'ok':True,'result':{'pid':os.getpid()}})
    elif f['kind']=='input': event(cid,f['input'])
    elif f['kind']=='event_ack': pass
    elif f['kind']=='cancel': send('cancelled',cid)
"#).unwrap();
    (fixture, executable)
}

#[tokio::test]
#[cfg(unix)]
async fn managed_duplex_raw_events_pipeline_and_cancel_preserves_neighbor() {
    use runtime_core::runtime_backend::RuntimeManagedDuplex;
    use std::time::Duration;
    let (_fixture, executable) = duplex_fixture();
    let mut workers = ManagedWorkers::default();
    let handle = mount_service(&mut workers, &executable, "service.duplex", Some(60000));
    let (input, receiver) = tokio::sync::mpsc::channel(8);
    let (events, mut output) = tokio::sync::mpsc::channel(8);
    let mut request = service_request(&handle, Some("actor"), serde_json::json!({"duplex":true}));
    request.duplex = Some(RuntimeManagedDuplex {
        input: Arc::new(tokio::sync::Mutex::new(receiver)),
        events,
    });
    let stream = tokio::spawn(workers.execute(request).unwrap());
    let first = tokio::time::timeout(Duration::from_secs(2), output.recv())
        .await
        .unwrap()
        .unwrap();
    let second = tokio::time::timeout(Duration::from_secs(2), output.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.event["pty_output"], "first");
    assert_eq!(second.event["pty_output"], "second");
    // Hold both consumption credits while sending Ctrl-C-like data.
    input.send(serde_json::json!("ctrl-c")).await.unwrap();
    let third = tokio::time::timeout(Duration::from_secs(2), output.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(third.event["pty_output"], "ctrl-c");
    let neighbor = workers
        .execute(service_request(
            &handle,
            Some("actor"),
            serde_json::json!({}),
        ))
        .unwrap()
        .await
        .unwrap();
    assert!(neighbor["pid"].as_u64().is_some());
    drop(input);
    assert!(tokio::time::timeout(Duration::from_secs(2), stream)
        .await
        .unwrap()
        .unwrap()
        .is_err());
    // Same actor carrier remains usable after this call's cancellation.
    let next = workers
        .execute(service_request(
            &handle,
            Some("actor"),
            serde_json::json!({}),
        ))
        .unwrap()
        .await
        .unwrap();
    assert_eq!(neighbor["pid"], next["pid"]);
    drop((first, second, third));
    workers
        .service_pool_for_handle(&handle)
        .unwrap()
        .unwrap()
        .retire_all()
        .await
        .unwrap();
}

#[tokio::test]
#[cfg(unix)]
async fn managed_duplex_drain_cancels_before_waiting_for_stream_scope() {
    use runtime_core::runtime_backend::RuntimeManagedDuplex;
    use std::time::Duration;
    let (_fixture, executable) = duplex_fixture();
    let mut workers = ManagedWorkers::default();
    let handle = mount_service(&mut workers, &executable, "service.duplex", Some(60000));
    let (_input, receiver) = tokio::sync::mpsc::channel(8);
    let (events, mut output) = tokio::sync::mpsc::channel(8);
    let mut request = service_request(&handle, Some("actor"), serde_json::json!({"duplex":true}));
    request.duplex = Some(RuntimeManagedDuplex {
        input: Arc::new(tokio::sync::Mutex::new(receiver)),
        events,
    });
    let stream = tokio::spawn(workers.execute(request).unwrap());
    let event = tokio::time::timeout(Duration::from_secs(2), output.recv())
        .await
        .unwrap()
        .unwrap();
    let drain = workers.drain(std::slice::from_ref(&handle)).unwrap();
    assert!(workers
        .execute(service_request(
            &handle,
            Some("actor"),
            serde_json::json!({})
        ))
        .is_err());
    tokio::time::timeout(Duration::from_secs(2), drain.wait_drained())
        .await
        .unwrap()
        .unwrap();
    assert!(stream.await.unwrap().is_err());
    drop(event);
    workers
        .service_pool_for_handle(&handle)
        .unwrap()
        .unwrap()
        .retire_all()
        .await
        .unwrap();
}

use super::*;
use fixtures::*;

fn without(mut value: Value, fields: &[&str]) -> Value {
    for row in value.as_array_mut().unwrap() {
        let object = row.as_object_mut().unwrap();
        for field in fields {
            object.remove(*field);
        }
    }
    value
}

#[tokio::test]
async fn verified_v1_v2_migration_is_atomic_lossless_and_reversible_across_all_owners() {
    for version in [1, 2] {
        let f = fixture(version).await;
        let before_state = durable_state(&f).await;
        assert_eq!(
            migration::collect_proofs(&f.snapshot).unwrap(),
            vec![f.old.clone()]
        );
        let plan = migration::build_plan(&f.snapshot, &f.verified).unwrap();
        assert_eq!(plan.documents.len(), 5);
        assert_eq!(plan.contexts.len(), 2, "shared delta DAG is copied once");
        assert_eq!(plan.checkpoints.len(), 3);
        assert_eq!(plan.continuations.len(), 3);
        assert_eq!(
            plan.contexts[1].after_content, plan.contexts[1].before.content,
            "unchanged delta only remaps its parent"
        );
        migration::preview_verified_flow(&f.store, &f.snapshot, &f.verified, KEY)
            .await
            .unwrap();
        assert_eq!(
            durable_state(&f).await,
            before_state,
            "preview is write-free"
        );
        let receipt = migration::apply_verified_flow(&f.store, &f.snapshot, &f.verified, KEY)
            .await
            .unwrap();
        assert_eq!(receipt.contexts.len(), 2);
        let after = f
            .store
            .load_responses_history_flow(&f.flow, KEY)
            .await
            .unwrap();
        assert!(migration::collect_proofs(&after).unwrap().is_empty());
        assert!(migration::inspect_flow(&after)
            .unwrap()
            .public_rounds
            .iter()
            .all(|round| round.history == f.current));
        let old_index: std::collections::HashMap<_, _> = f
            .snapshot
            .checkpoints
            .iter()
            .map(|cp| (cp.checkpoint_id, cp))
            .collect();
        for cp in &after.checkpoints {
            let mut expected = materialized(&f.snapshot, old_index[&cp.checkpoint_id]);
            expected["sys"]["responses_input_history"] = f.current.clone();
            expected["sys"]["native_inference_recovery"]["history"] = f.current.clone();
            expected["node-llm"]["__llm_tool_callback"]["provider_metadata"]["native_response"]
                ["history"] = f.current.clone();
            expected["node-llm"]["__llm_tool_callback"]["responses_round"]["history"] =
                f.current.clone();
            assert_eq!(
                materialized(&after, cp),
                expected,
                "materialized user pool changes only approved proof leaves"
            );
            assert_eq!(
                cp.variable_snapshot.as_object().unwrap().len(),
                1,
                "mutable snapshot is marker-only"
            );
            assert_eq!(
                materialized(&after, cp)["user"]["history"],
                f.old,
                "user proof-looking JSON is opaque"
            );
        }
        assert_eq!(
            after.checkpoints[1].context, after.checkpoints[2].context,
            "shared aliases map to one context"
        );
        for slot in &after.continuations {
            let old = f
                .snapshot
                .continuations
                .iter()
                .find(|old| old.slot_key == slot.slot_key)
                .unwrap();
            assert_eq!(slot.payload["native_history"], f.current);
            assert_ne!(slot.encrypted_payload, old.encrypted_payload);
            assert_eq!(slot.hard_expires_at, old.hard_expires_at);
            assert_eq!(slot.key_version, old.key_version);
        }
        assert!(
            after
                .continuations
                .iter()
                .any(|slot| slot.hard_expires_at < OffsetDateTime::now_utc()),
            "expired retained claimed slots must not be cleaned up"
        );
        let after_state = durable_state(&f).await;
        let node_input:Value=sqlx::query_scalar("select runtime_original_json(input_payload,raw_json_payloads,'input_payload') from node_run_records where id=$1").bind(f.node_id).fetch_one(f.store.pool()).await.unwrap();
        assert_eq!(
            node_input,
            json!({"user":"keep\0node"}),
            "writing node output must retain the original input sidecar"
        );
        for table in [
            "runtime_invocation_context_bindings",
            "flow_run_recovery_history",
            "flow_run_tool_callback_inbox",
            "flow_run_callback_resume_attempts",
            "flow_run_resume_claims",
            "runtime_events",
        ] {
            assert_eq!(
                after_state[table], before_state[table],
                "business and audit owner {table} must stay untouched"
            );
        }
        assert_eq!(
            without(
                after_state["flow_run_callback_tasks"].clone(),
                &["request_payload", "raw_json_payloads"]
            ),
            without(
                before_state["flow_run_callback_tasks"].clone(),
                &["request_payload", "raw_json_payloads"]
            )
        );
        assert_eq!(
            without(
                after_state["node_run_records"].clone(),
                &["output_payload", "raw_json_payloads"]
            ),
            without(
                before_state["node_run_records"].clone(),
                &["output_payload", "raw_json_payloads"]
            )
        );
        for table in ["runtime_canonical_contents", "runtime_context_projections"] {
            for old in before_state[table].as_array().unwrap() {
                let stored = after_state[table]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|row| row["id"] == old["id"])
                    .unwrap();
                assert_eq!(stored, old, "original immutable row must not be rewritten");
            }
        }
        let shadow=sqlx::query("select s.context_version_id,s.canonical_content_id,runtime_original_json(c.content,c.raw_json_payloads,'content') as content,c.content_hash,s.source_hash,c.byte_size,s.source_byte_size from runtime_legacy_shadow_rows s join runtime_canonical_contents c on c.id=s.canonical_content_id where s.source_table='flow_run_checkpoints' and s.source_row_id=$1")
            .bind(after.checkpoints[0].checkpoint_id).fetch_one(f.store.pool()).await.unwrap();
        assert_eq!(
            shadow
                .try_get::<Option<Uuid>, _>("context_version_id")
                .unwrap(),
            None
        );
        assert_eq!(
            shadow.try_get::<Value, _>("content").unwrap(),
            after.checkpoints[0].variable_snapshot
        );
        assert_ne!(
            shadow.try_get::<Uuid, _>("canonical_content_id").unwrap(),
            after.checkpoints[0]
                .context
                .as_ref()
                .unwrap()
                .actual_content_id
        );
        assert_eq!(
            shadow.try_get::<String, _>("source_hash").unwrap(),
            shadow.try_get::<String, _>("content_hash").unwrap()
        );
        assert_eq!(
            shadow.try_get::<i64, _>("source_byte_size").unwrap(),
            shadow.try_get::<i64, _>("byte_size").unwrap()
        );
        // A controlled stale shadow must be rejected rather than silently repaired.
        let hash: String = shadow.try_get("source_hash").unwrap();
        sqlx::query("update runtime_legacy_shadow_rows set source_hash=$2 where source_table='flow_run_checkpoints' and source_row_id=$1").bind(after.checkpoints[0].checkpoint_id).bind(format!("sha256:{}","0".repeat(64))).execute(f.store.pool()).await.unwrap();
        assert_apply_refused_unchanged(&f, &plan).await;
        sqlx::query("update runtime_legacy_shadow_rows set source_hash=$2 where source_table='flow_run_checkpoints' and source_row_id=$1").bind(after.checkpoints[0].checkpoint_id).bind(hash).execute(f.store.pool()).await.unwrap();
        let replay = migration::apply_verified_flow(&f.store, &f.snapshot, &f.verified, KEY)
            .await
            .unwrap();
        assert!(
            replay.contexts.is_empty()
                && replay.documents.is_empty()
                && replay.continuations.is_empty()
        );
        assert_eq!(
            durable_state(&f).await,
            after_state,
            "idempotent reapply does not append content or reseal ciphertext"
        );
        f.store
            .rollback_responses_history_migration(&receipt, KEY)
            .await
            .unwrap();
        let restored = f
            .store
            .load_responses_history_flow(&f.flow, KEY)
            .await
            .unwrap();
        assert_eq!(restored.documents, f.snapshot.documents);
        assert_eq!(restored.checkpoints, f.snapshot.checkpoints);
        assert_eq!(restored.continuations, f.snapshot.continuations);
        for (old, new) in f.snapshot.checkpoints.iter().zip(&restored.checkpoints) {
            assert_eq!(materialized(&restored, new), materialized(&f.snapshot, old));
        }
        let old_head = f.snapshot.contexts.last().unwrap();
        let restored_head = restored.contexts.last().unwrap();
        assert_eq!(restored_head.content, old_head.content);
        assert_eq!(
            restored_head.previous_projection_id,
            old_head.previous_projection_id
        );
        assert_ne!(
            restored.latest_context_version_id, f.snapshot.latest_context_version_id,
            "rollback restores effective data with new immutable identity"
        );
        let rollback_state = durable_state(&f).await;
        f.store
            .rollback_responses_history_migration(&receipt, KEY)
            .await
            .unwrap();
        assert_eq!(
            durable_state(&f).await,
            rollback_state,
            "repeated rollback is a no-op"
        );
    }
}

#[tokio::test]
async fn indexed_original_native_and_public_sources_produce_verified_receipts() {
    let f = fixture(2).await;
    let mut graph =
        migration::HistorySourceGraph::new(&migration::collect_proofs(&f.snapshot).unwrap())
            .unwrap();
    let mut after_sequence = None;
    let mut sources = Vec::new();
    loop {
        let page = f
            .store
            .list_responses_history_native_sources(&ResponsesHistoryNativeSourcePageInput {
                flow: f.flow,
                after_sequence,
                limit: 1,
            })
            .await
            .unwrap();
        for source in page.sources {
            graph
                .push_native(
                    f.flow.flow_run_id,
                    source.node_run_id,
                    source.id,
                    source.sequence,
                    &source.event_type,
                    &source.payload,
                )
                .unwrap();
            sources.push(source);
        }
        after_sequence = page.next_after_sequence;
        if after_sequence.is_none() {
            break;
        }
    }
    assert_eq!(sources.len(), 3);
    assert!(sources
        .iter()
        .all(|source| source.node_run_id == Some(f.node_id)));
    graph.finish_native_flow(f.flow.flow_run_id).unwrap();
    let resolved = graph.resolve().unwrap();
    assert!(resolved.unresolved_sources.is_empty() && resolved.unresolved_targets.is_empty());
    assert_eq!(resolved.verified[0].replacement.old(), f.old);
    assert_eq!(resolved.verified[0].replacement.current(), f.current);
    let original: Value =
        serde_json::from_str(sources[0].payload["body"].as_str().unwrap()).unwrap();
    assert_eq!(
        original["native_request"]["wire_body"]["input"][0]["content"],
        "original\0user line"
    );
    let inspection = migration::inspect_flow(&f.snapshot).unwrap();
    let round = &inspection.public_rounds[0];
    let mut public = migration::HistorySourceGraph::new(&[f.old.clone()]).unwrap();
    public.push_public(f.flow.flow_run_id,sources[0].id,&json!({"encoding":"utf8","frame_kind":"request","body":original["native_request"]["wire_body"].to_string()}),round).unwrap();
    let public = public.resolve().unwrap();
    assert!(public.unresolved_sources.is_empty() && public.unresolved_targets.is_empty());
    assert_eq!(public.verified[0].replacement.current(), f.current);
    assert_eq!(
        public.verified[0].source.namespace,
        migration::SourceNamespace::Public
    );
    let mut wrong = migration::MigrationAccumulator::new(2).unwrap();
    wrong
        .append(&json!({"role":"user","content":"different"}))
        .unwrap();
    assert!(wrong.verify(&f.old).is_err());
    let before = durable_state(&f).await;
    assert!(
        migration::apply_verified_flow(&f.store, &f.snapshot, &[], KEY)
            .await
            .is_err(),
        "missing verified evidence must not relabel old proof"
    );
    assert_eq!(durable_state(&f).await, before);
    let page = f
        .store
        .list_responses_history_flows(&ResponsesHistoryFlowPageInput {
            scope_id: f.flow.scope_id,
            application_id: f.flow.application_id,
            after_flow_run_id: None,
            limit: 1001,
        })
        .await
        .unwrap();
    assert_eq!(
        page.flows,
        vec![f.flow],
        "operational page size has no guessed 1000 ceiling"
    );
}

#[tokio::test]
async fn wrong_scope_owner_live_claim_stale_preimage_and_mixed_state_refuse_atomically() {
    let f = fixture(2).await;
    let plan = migration::build_plan(&f.snapshot, &f.verified).unwrap();
    let mut wrong = plan.clone();
    wrong.flow.scope_id = Uuid::now_v7();
    assert_apply_refused_unchanged(&f, &wrong).await;
    let mut owner = plan.clone();
    owner.documents[0].owner = ResponsesHistoryDocumentOwner::NodeOutput {
        node_run_id: Uuid::now_v7(),
    };
    assert_apply_refused_unchanged(&f, &owner).await;
    sqlx::query("update flow_runs set status='running' where id=$1")
        .bind(f.flow.flow_run_id)
        .execute(f.store.pool())
        .await
        .unwrap();
    let mut running = plan.clone();
    running.expected_status = domain::FlowRunStatus::Running;
    assert_apply_refused_unchanged(&f, &running).await;
    sqlx::query("update flow_runs set status='paused' where id=$1")
        .bind(f.flow.flow_run_id)
        .execute(f.store.pool())
        .await
        .unwrap();
    let mut stale = plan.clone();
    stale.documents[0].before["later"] = json!(true);
    assert_apply_refused_unchanged(&f, &stale).await;
    sqlx::query("insert into flow_run_resume_claims(id,scope_id,application_id,flow_run_id,checkpoint_id,callback_task_id,resume_kind,status,request_payload,claim_token,lease_expires_at) values($1,$2,$3,$4,$5,$6,'callback','processing','{}'::jsonb,$7,now()+interval '5 minutes')")
        .bind(Uuid::now_v7()).bind(f.flow.scope_id).bind(f.flow.application_id).bind(f.flow.flow_run_id).bind(f.snapshot.checkpoints[0].checkpoint_id).bind(f.callback_id).bind(Uuid::now_v7()).execute(f.store.pool()).await.unwrap();
    assert_apply_refused_unchanged(&f, &plan).await;
    sqlx::query("update flow_run_resume_claims set lease_expires_at=now()-interval '1 minute' where flow_run_id=$1").bind(f.flow.flow_run_id).execute(f.store.pool()).await.unwrap();
    let receipt = migration::apply_verified_flow(&f.store, &f.snapshot, &f.verified, KEY)
        .await
        .unwrap();
    let after = f
        .store
        .load_responses_history_flow(&f.flow, KEY)
        .await
        .unwrap();
    let before_cp = &f.snapshot.checkpoints[1];
    f.store
        .update_checkpoint_payloads(&UpdateCheckpointPayloadsInput {
            checkpoint_id: before_cp.checkpoint_id,
            locator_payload: before_cp.locator_payload.clone(),
            variable_snapshot: before_cp.variable_snapshot.clone(),
            external_ref_payload: None,
        })
        .await
        .unwrap();
    assert_apply_refused_unchanged(&f, &plan).await;
    let after_cp = &after.checkpoints[1];
    f.store
        .update_checkpoint_payloads(&UpdateCheckpointPayloadsInput {
            checkpoint_id: after_cp.checkpoint_id,
            locator_payload: after_cp.locator_payload.clone(),
            variable_snapshot: after_cp.variable_snapshot.clone(),
            external_ref_payload: None,
        })
        .await
        .unwrap();
    f.store
        .rollback_responses_history_migration(&receipt, KEY)
        .await
        .unwrap();
}

#[tokio::test]
async fn malformed_cipher_key_digest_and_missing_slots_abort_every_owner() {
    let f = fixture(2).await;
    let plan = migration::build_plan(&f.snapshot, &f.verified).unwrap();
    let slot = f
        .snapshot
        .continuations
        .iter()
        .find(|slot| slot.slot_key == "current")
        .unwrap();
    for variant in 0..6 {
        match variant {
            0 => {
                sqlx::query("update provider_protocol_capsules set encrypted_payload=jsonb_set(encrypted_payload,'{ciphertext}','\"00\"'::jsonb) where flow_run_id=$1 and slot_key=$2").bind(f.flow.flow_run_id).bind(&slot.slot_key).execute(f.store.pool()).await.unwrap();
            }
            1 => {
                sqlx::query("update provider_protocol_capsules set key_version='unsupported:v99' where flow_run_id=$1 and slot_key=$2").bind(f.flow.flow_run_id).bind(&slot.slot_key).execute(f.store.pool()).await.unwrap();
            }
            2 => {
                sqlx::query("update provider_protocol_capsules set plaintext_digest='sha256:tampered' where flow_run_id=$1 and slot_key=$2").bind(f.flow.flow_run_id).bind(&slot.slot_key).execute(f.store.pool()).await.unwrap();
            }
            3 => {
                let other = f
                    .snapshot
                    .continuations
                    .iter()
                    .find(|other| other.slot_key != "current")
                    .unwrap();
                sqlx::query("update provider_protocol_capsules set encrypted_payload=$3 where flow_run_id=$1 and slot_key=$2").bind(f.flow.flow_run_id).bind(&slot.slot_key).bind(&other.encrypted_payload).execute(f.store.pool()).await.unwrap();
            }
            4 => {
                sqlx::query("update provider_protocol_capsules set plaintext_size_bytes=plaintext_size_bytes+1 where flow_run_id=$1 and slot_key=$2").bind(f.flow.flow_run_id).bind(&slot.slot_key).execute(f.store.pool()).await.unwrap();
            }
            _ => {
                sqlx::query(
                    "delete from provider_protocol_capsules where flow_run_id=$1 and slot_key=$2",
                )
                .bind(f.flow.flow_run_id)
                .bind(&slot.slot_key)
                .execute(f.store.pool())
                .await
                .unwrap();
            }
        }
        assert_apply_refused_unchanged(&f, &plan).await;
        sqlx::query("insert into provider_protocol_capsules(flow_run_id,capsule_kind,slot_key,encrypted_payload,plaintext_digest,plaintext_size_bytes,key_version,hard_expires_at) values($1,'continuation',$2,$3,$4,$5,$6,$7) on conflict(flow_run_id,capsule_kind,slot_key) do update set encrypted_payload=excluded.encrypted_payload,plaintext_digest=excluded.plaintext_digest,plaintext_size_bytes=excluded.plaintext_size_bytes,key_version=excluded.key_version,hard_expires_at=excluded.hard_expires_at")
            .bind(f.flow.flow_run_id).bind(&slot.slot_key).bind(&slot.encrypted_payload).bind(&slot.plaintext_digest).bind(slot.plaintext_size_bytes).bind(&slot.key_version).bind(slot.hard_expires_at).execute(f.store.pool()).await.unwrap();
    }
    let before = durable_state(&f).await;
    assert!(
        migration::apply_verified_flow(&f.store, &f.snapshot, &f.verified, "wrong-master-key")
            .await
            .is_err()
    );
    assert_eq!(durable_state(&f).await, before);
}

#[tokio::test]
async fn corrupt_canonical_preimage_is_detected_without_partial_migration() {
    let f = fixture(2).await;
    let plan = migration::build_plan(&f.snapshot, &f.verified).unwrap();
    let source = f.snapshot.contexts[0].identity.actual_content_id;
    // Controlled negative on this fixture's isolated schema only, mirroring the
    // existing storage_foundation collision test. Re-enable immutability first.
    sqlx::query("alter table runtime_canonical_contents disable trigger runtime_canonical_contents_reject_update").execute(f.store.pool()).await.unwrap();
    sqlx::query("update runtime_canonical_contents set content=content||'{\"corrupt\":true}'::jsonb,raw_json_payloads='{}'::jsonb where id=$1").bind(source).execute(f.store.pool()).await.unwrap();
    sqlx::query("alter table runtime_canonical_contents enable trigger runtime_canonical_contents_reject_update").execute(f.store.pool()).await.unwrap();
    assert_apply_refused_unchanged(&f, &plan).await;
}

#[tokio::test]
async fn rollback_refuses_later_document_change_without_touching_other_owners() {
    let f = fixture(2).await;
    let receipt = migration::apply_verified_flow(&f.store, &f.snapshot, &f.verified, KEY)
        .await
        .unwrap();
    let after = f
        .store
        .load_responses_history_flow(&f.flow, KEY)
        .await
        .unwrap();
    let input = after
        .documents
        .iter()
        .find(|doc| doc.owner == ResponsesHistoryDocumentOwner::FlowInput)
        .unwrap();
    let output = after
        .documents
        .iter()
        .find(|doc| doc.owner == ResponsesHistoryDocumentOwner::FlowOutput)
        .unwrap();
    let mut changed = input.value.clone();
    changed["user"]["later"] = json!(true);
    f.store
        .update_flow_run_payloads(&UpdateFlowRunPayloadsInput {
            flow_run_id: f.flow.flow_run_id,
            input_payload: changed,
            output_payload: output.value.clone(),
            error_payload: None,
        })
        .await
        .unwrap();
    let before = durable_state(&f).await;
    assert!(f
        .store
        .rollback_responses_history_migration(&receipt, KEY)
        .await
        .is_err());
    assert_eq!(durable_state(&f).await, before);
    f.store
        .update_flow_run_payloads(&UpdateFlowRunPayloadsInput {
            flow_run_id: f.flow.flow_run_id,
            input_payload: input.value.clone(),
            output_payload: output.value.clone(),
            error_payload: None,
        })
        .await
        .unwrap();
    f.store
        .rollback_responses_history_migration(&receipt, KEY)
        .await
        .unwrap();
    assert_eq!(
        f.store
            .load_responses_history_flow(&f.flow, KEY)
            .await
            .unwrap()
            .documents,
        f.snapshot.documents
    );
}

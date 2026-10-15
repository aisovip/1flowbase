use super::*;
pub(super) const KEY: &str = "responses-history-isolated-fixture-key";

pub(super) struct Fixture {
    pub store: PgControlPlaneStore,
    pub flow: ResponsesHistoryFlowKey,
    pub node_id: Uuid,
    pub callback_id: Uuid,
    pub old: Value,
    pub current: Value,
    pub verified: Vec<migration::SourcedReplacement>,
    pub snapshot: ResponsesHistoryFlowSnapshot,
}

async fn seed() -> (PgControlPlaneStore, ResponsesHistoryFlowKey) {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:1flowbase@127.0.0.1:35432/1flowbase".into());
    let schema = postgres_test_support::PostgresTestSchema::create(&url)
        .await
        .unwrap();
    // connect transfers the temporary database guard into the pool lifetime.
    let pool = schema.connect().await.unwrap();
    run_migrations(&pool).await.unwrap();
    let store = PgControlPlaneStore::new(pool.clone());
    let tenant_id: Uuid = sqlx::query_scalar("select id from tenants where code = 'root-tenant'")
        .fetch_one(store.pool())
        .await
        .unwrap();
    let workspace_id = Uuid::now_v7();
    sqlx::query("insert into workspaces (id, tenant_id, name) values ($1, $2, 'Capsule fixture')")
        .bind(workspace_id)
        .bind(tenant_id)
        .execute(store.pool())
        .await
        .unwrap();
    let user_id = Uuid::now_v7();
    let account = format!("capsule-{}", user_id.simple());
    sqlx::query(
        r#"
        insert into users (
            id, account, email, password_hash, name, nickname, introduction,
            default_display_role, email_login_enabled, phone_login_enabled, status,
            session_version
        ) values ($1, $2, $3, 'hash', $2, $2, '', 'member', true, false, 'active', 1)
        "#,
    )
    .bind(user_id)
    .bind(&account)
    .bind(format!("{account}@example.com"))
    .execute(store.pool())
    .await
    .unwrap();
    let application_id = Uuid::now_v7();
    sqlx::query(
        "insert into applications (id, workspace_id, application_type, name, description, created_by, updated_by) values ($1, $2, 'agent_flow', 'Capsule fixture', '', $3, $3)",
    )
    .bind(application_id)
    .bind(workspace_id)
    .bind(user_id)
    .execute(store.pool())
    .await
    .unwrap();
    let flow_id = Uuid::now_v7();
    sqlx::query(
        "insert into flows (id, application_id, scope_id, created_by, updated_by) values ($1, $2, (select scope_id from applications where id = $2), $3, $3)",
    )
    .bind(flow_id)
    .bind(application_id)
    .bind(user_id)
    .execute(store.pool())
    .await
    .unwrap();
    let draft_id = Uuid::now_v7();
    sqlx::query(
        "insert into flow_drafts (id, flow_id, scope_id, schema_version, document, created_by, updated_by) values ($1, $2, (select scope_id from flows where id = $2), $3, '{}'::jsonb, $4, $4)",
    )
    .bind(draft_id)
    .bind(flow_id)
    .bind(domain::FLOW_SCHEMA_VERSION)
    .bind(user_id)
    .execute(store.pool())
    .await
    .unwrap();
    let plan_id = Uuid::now_v7();
    sqlx::query(
        "insert into flow_compiled_plans (id, flow_id, flow_draft_id, schema_version, document_updated_at, plan, scope_id, created_by, updated_by) values ($1, $2, $3, $4, now(), '{}'::jsonb, (select scope_id from flows where id = $2), $5, $5)",
    )
    .bind(plan_id)
    .bind(flow_id)
    .bind(draft_id)
    .bind(domain::FLOW_SCHEMA_VERSION)
    .bind(user_id)
    .execute(store.pool())
    .await
    .unwrap();
    let flow_run_id = Uuid::now_v7();
    sqlx::query(
        "insert into flow_runs (id, application_id, flow_id, flow_draft_id, compiled_plan_id, run_mode, status, created_by) values ($1, $2, $3, $4, $5, 'published_api_run', 'paused', $6)",
    )
    .bind(flow_run_id)
    .bind(application_id)
    .bind(flow_id)
    .bind(draft_id)
    .bind(plan_id)
    .bind(user_id)
    .execute(store.pool())
    .await
    .unwrap();

    (
        store,
        ResponsesHistoryFlowKey {
            scope_id: workspace_id,
            application_id,
            flow_run_id,
        },
    )
}

pub(super) fn round(proof: &Value, output: &Value) -> Value {
    json!({"response_id":"fixture-response","history":proof,"output":[output]})
}

pub(super) fn callback(proof: &Value, output: &Value) -> Value {
    json!({"provider_metadata":{"native_response":{"response_id":"fixture-response","history":proof}},"responses_round":round(proof,output),"tool_calls":[{"id":"call_fixture","name":"exec"}],"history":[{"role":"tool","content":{"history":proof,"user":"keep\0opaque"}}]})
}

async fn context(
    store: &PgControlPlaneStore,
    flow: &ResponsesHistoryFlowKey,
    content: Value,
    parent: Option<Uuid>,
) -> domain::ContextVersionRecord {
    let content = store
        .put_canonical_runtime_content(&PutCanonicalRuntimeContentInput {
            scope_id: flow.scope_id,
            application_id: flow.application_id,
            content,
        })
        .await
        .unwrap();
    let sequence:i64=sqlx::query_scalar("select coalesce(max(context_sequence),-1)+1 from runtime_context_projections where flow_run_id=$1").bind(flow.flow_run_id).fetch_one(store.pool()).await.unwrap();
    store
        .append_context_version(&AppendContextVersionInput {
            scope_id: flow.scope_id,
            application_id: flow.application_id,
            flow_run_id: flow.flow_run_id,
            parent_context_version_id: parent,
            sequence,
            transition_kind: ContextTransitionKind::Append,
            transition_actor: ContextTransitionActor::Host,
            declared_compaction_provenance: None,
            actual_content_id: content.id,
        })
        .await
        .unwrap()
}

async fn event(
    store: &PgControlPlaneStore,
    flow: Uuid,
    node: Uuid,
    event_type: &str,
    mut payload: Value,
) {
    if event_type == "provider_semantic_step" {
        payload["step_key"] = json!(format!(
            "{}:{}",
            payload["invocation_id"], payload["direction"]
        ));
    }
    store
        .append_runtime_event(&AppendRuntimeEventInput {
            flow_run_id: flow,
            node_run_id: Some(node),
            span_id: None,
            parent_span_id: None,
            event_type: event_type.into(),
            layer: domain::RuntimeEventLayer::ProviderRaw,
            source: domain::RuntimeEventSource::Host,
            trust_level: domain::RuntimeTrustLevel::HostFact,
            item_id: None,
            ledger_ref: None,
            payload,
            visibility: domain::RuntimeEventVisibility::Workspace,
            durability: domain::RuntimeEventDurability::Durable,
        })
        .await
        .unwrap();
}

pub(super) async fn fixture(version: u32) -> Fixture {
    fixture_with_output(version,json!({"type":"function_call","id":"fc_fixture","call_id":"call_fixture","name":"exec","arguments":"{ \"n\": 9007199254740993 }"})).await
}

pub(super) async fn fixture_with_output(version: u32, output: Value) -> Fixture {
    let (store, flow) = seed().await;
    let user = json!({"role":"user","content":"original\0user line"});
    let mut accumulator = migration::MigrationAccumulator::new(version).unwrap();
    accumulator.append(&user).unwrap();
    accumulator.append(&output).unwrap();
    let old = accumulator.snapshot().old();
    // The complete legacy proof is verified from original source items; new
    // digests are never fabricated or accepted as trusted deserialized receipts.
    let verified_receipt = accumulator.verify(&old).unwrap();
    let current = verified_receipt.current();
    let node = store
        .create_node_run(&CreateNodeRunInput {
            flow_run_id: flow.flow_run_id,
            node_id: "node-llm".into(),
            node_type: "llm".into(),
            node_alias: "LLM".into(),
            status: NodeRunStatus::WaitingCallback,
            input_payload: json!({"user":"keep\0node"}),
            debug_payload: json!({}),
            started_at: OffsetDateTime::now_utc(),
        })
        .await
        .unwrap();
    let body = callback(&old, &output);
    let callback = store
        .create_callback_task(&CreateCallbackTaskInput {
            flow_run_id: flow.flow_run_id,
            node_run_id: node.id,
            callback_kind: "llm_tool".into(),
            request_payload: body.clone(),
            external_ref_payload: Some(json!({"callback":"owned"})),
        })
        .await
        .unwrap();
    let pool = json!({"sys":{"responses_input_history":old,"native_inference_recovery":{"history":old}},"node-llm":{"__llm_tool_callback":body},"user":{"history":old,"note":"user\0raw","number":9007199254740993i64}});
    store.update_flow_run_payloads(&UpdateFlowRunPayloadsInput {flow_run_id:flow.flow_run_id,input_payload:json!({"sys":{"responses_input_history":old,"native_inference_recovery":{"history":old}},"user":{"history":old,"raw":"flow\0source"}}),output_payload:json!({"responses_round":round(&old,&output),"user":"output\0raw"}),error_payload:None}).await.unwrap();
    store
        .update_node_run_payloads(&UpdateNodeRunPayloadsInput {
            node_run_id: node.id,
            input_payload: node.input_payload.clone(),
            output_payload: body.clone(),
            error_payload: None,
            metrics_payload: json!({}),
            debug_payload: json!({}),
        })
        .await
        .unwrap();
    let inline = store
        .create_checkpoint(&CreateCheckpointInput {
            flow_run_id: flow.flow_run_id,
            node_run_id: Some(node.id),
            status: "paused".into(),
            reason: "legacy source".into(),
            locator_payload: json!({"node_id":"node-llm","opaque":"locator\0source"}),
            variable_snapshot: pool.clone(),
            external_ref_payload: None,
        })
        .await
        .unwrap();
    let shadow = store
        .convert_legacy_runtime_shadow_batch(&ConvertLegacyRuntimeShadowBatchInput {
            application_id: Some(flow.application_id),
            flow_run_id: Some(flow.flow_run_id),
            after: None,
            limit: 100,
            lock_budget_ms: 1000,
            execution: LegacyRuntimeShadowExecution::Apply,
        })
        .await
        .unwrap();
    assert!(shadow.differences.is_empty());
    let initial = context(
        &store,
        &flow,
        json!({"format":"runtime_snapshot_v1","variable_pool":pool}),
        None,
    )
    .await;
    let delta=context(&store,&flow,json!({"format":"runtime_delta_v1","set":{"answer":{"user":"delta\0value"}},"remove":[],"llm_callback_appends":[]}),Some(initial.id)).await;
    for _ in 0..2 {
        store.create_checkpoint(&CreateCheckpointInput {flow_run_id:flow.flow_run_id,node_run_id:Some(node.id),status:"paused".into(),reason:"shared delta head".into(),locator_payload:json!({"node_id":"node-llm","context_version_id":delta.id}),variable_snapshot:json!({"__runtime_recovery_context":{"context_version_id":delta.id,"sequence":delta.sequence}}),external_ref_payload:None}).await.unwrap();
    }
    let span=store.append_runtime_span(&AppendRuntimeSpanInput {flow_run_id:flow.flow_run_id,node_run_id:Some(node.id),parent_span_id:None,kind:domain::RuntimeSpanKind::LlmTurn,name:"proof metadata".into(),status:domain::RuntimeSpanStatus::Succeeded,capability_id:None,input_ref:None,output_ref:None,error_payload:None,metadata:json!({"native_response":{"history":old},"user":{"history":old,"raw":"span\0raw"}}),started_at:OffsetDateTime::now_utc(),finished_at:Some(OffsetDateTime::now_utc())}).await.unwrap();
    store
        .bind_invocation_context(&BindInvocationContextInput {
            invocation_span_id: span.id,
            scope_id: flow.scope_id,
            application_id: flow.application_id,
            flow_run_id: flow.flow_run_id,
            context_version_id: delta.id,
        })
        .await
        .unwrap();
    store
        .append_recovery_history(&AppendRecoveryHistoryInput {
            scope_id: flow.scope_id,
            application_id: flow.application_id,
            flow_run_id: flow.flow_run_id,
            node_run_id: Some(node.id),
            sequence: 0,
            state_code: RecoveryStateCode::WaitingCallback,
            coordinate: RecoveryCoordinate {
                node_sequence: 0,
                iteration_index: 0,
                attempt_index: 0,
                resume_sequence: 0,
                event_sequence: 0,
            },
            context_version_id: delta.id,
            recovery_content_id: Some(delta.actual_content_id),
            idempotency_key: "history-fixture-original".into(),
        })
        .await
        .unwrap();
    store.record_flow_run_callback_resume_attempt(&RecordFlowRunCallbackResumeAttemptInput {flow_run_id:flow.flow_run_id,callback_task_id:callback.id,source:"history-fixture".into(),response_payload:json!({"tool_results":[{"tool_call_id":"call_fixture","content":"original"}]}),idempotency_key:"history-fixture-original-attempt".into()}).await.unwrap();
    sqlx::query("insert into flow_run_tool_callback_inbox(id,scope_id,application_id,flow_run_id,callback_task_id,tool_call_id,tool_ordinal) values($1,$2,$3,$4,$5,'call_fixture',0) on conflict do nothing").bind(Uuid::now_v7()).bind(flow.scope_id).bind(flow.application_id).bind(flow.flow_run_id).bind(callback.id).execute(store.pool()).await.unwrap();
    let capsules =
        PgProviderProtocolCapsuleStore::new(store.pool().clone(), KEY, Duration::days(7)).unwrap();
    for slot in [
        ProviderContinuationSlotId::for_flow_run(flow.flow_run_id),
        ProviderContinuationSlotId::for_response_round(flow.flow_run_id, Uuid::now_v7()),
        ProviderContinuationSlotId::for_resume_claim(flow.flow_run_id, Uuid::now_v7()),
    ] {
        capsules
            .put_continuation(
                slot,
                ProviderContinuation::new(
                    "fixture-response",
                    ProviderTransportAffinity::new(
                        "instance",
                        "openai",
                        "openai_responses",
                        "model",
                    ),
                )
                .unwrap()
                .with_native_history(Some(old.clone())),
            )
            .await
            .unwrap();
    }
    sqlx::query("update provider_protocol_capsules set hard_expires_at=now()-interval '1 day' where flow_run_id=$1 and slot_key like 'claim:%'").bind(flow.flow_run_id).execute(store.pool()).await.unwrap();
    let invocation = Uuid::now_v7();
    event(&store,flow.flow_run_id,node.id,"provider_semantic_step",json!({"source":"ai_native","kind":"model_call","direction":"prepared","status":"recorded","invocation_id":invocation,"provider_attempt_index":0,"body":json!({"native_request":{"wire_body":{"input":[user]}}}).to_string()})).await;
    event(
        &store,
        flow.flow_run_id,
        node.id,
        "provider_output_item_done",
        json!({"output_index":0,"item":output}),
    )
    .await;
    event(&store,flow.flow_run_id,node.id,"provider_semantic_step",json!({"source":"ai_native","kind":"model_reply","direction":"received","status":"recorded","invocation_id":invocation,"provider_attempt_index":0,"body":json!({"output_items":[],"result":{"response_id":"fixture-response","finish_reason":"tool_call"}}).to_string()})).await;
    let sources = store
        .list_responses_history_native_sources(&ResponsesHistoryNativeSourcePageInput {
            flow,
            after_sequence: None,
            limit: 10,
        })
        .await
        .unwrap();
    assert_eq!(sources.sources.len(), 3);
    let mut graph = migration::HistorySourceGraph::new(&[migration::ScopedHistoryTarget {
        flow_run_id: flow.flow_run_id,
        proof: old.clone(),
    }])
    .unwrap();
    for source in &sources.sources {
        graph
            .push_native(
                flow.flow_run_id,
                source.node_run_id,
                source.id,
                source.sequence,
                &source.event_type,
                &source.payload,
            )
            .unwrap();
    }
    graph.finish_native_flow(flow.flow_run_id).unwrap();
    let resolution = graph.resolve().unwrap();
    assert!(resolution.unresolved_targets.is_empty() && resolution.unresolved_sources.is_empty());
    assert_eq!(resolution.verified.len(), 1);
    let verified = resolution.verified;
    assert_eq!(verified[0].replacement().old(), old);
    assert_eq!(verified[0].replacement().current(), current);
    assert_eq!(verified[0].source().flow_run_id, flow.flow_run_id);
    let snapshot = store.load_responses_history_flow(&flow, KEY).await.unwrap();
    assert_eq!(snapshot.contexts.len(), 2);
    assert_eq!(snapshot.checkpoints[0].checkpoint_id, inline.id);
    Fixture {
        store,
        flow,
        node_id: node.id,
        callback_id: callback.id,
        old,
        current,
        verified,
        snapshot,
    }
}

pub(super) fn materialized(
    snapshot: &ResponsesHistoryFlowSnapshot,
    checkpoint: &ResponsesHistoryCheckpoint,
) -> Value {
    if checkpoint.context.is_none() {
        return checkpoint.variable_snapshot.clone();
    }
    let index: std::collections::HashMap<_, _> = snapshot
        .contexts
        .iter()
        .map(|c| (c.identity.context_version_id, c))
        .collect();
    let mut next = checkpoint.context.as_ref().map(|id| id.context_version_id);
    let mut lineage = Vec::new();
    while let Some(id) = next {
        let entry = index[&id];
        lineage.push(RuntimeContextContentVersion {
            context_version_id: id,
            sequence: entry.sequence,
            content: entry.content.clone(),
        });
        next = entry.previous_projection_id;
    }
    lineage.reverse();
    let mut pool = migration::materialize_checkpoint_content(&lineage).unwrap();
    pool.remove("__runtime_recovery_context");
    Value::Object(pool)
}

pub(super) async fn durable_state(f: &Fixture) -> Value {
    let mut result = serde_json::Map::new();
    for (table, predicate, order) in [
        ("flow_runs", "t.id=$1", "t.id"),
        ("node_run_records", "t.flow_run_id=$1", "t.id"),
        ("flow_run_callback_tasks", "t.flow_run_id=$1", "t.id"),
        ("flow_run_checkpoints", "t.flow_run_id=$1", "t.id"),
        ("runtime_spans", "t.flow_run_id=$1", "t.id"),
        (
            "provider_protocol_capsules",
            "t.flow_run_id=$1",
            "t.slot_key",
        ),
        ("runtime_legacy_shadow_rows", "t.flow_run_id=$1", "t.id"),
        ("runtime_context_projections", "t.flow_run_id=$1", "t.id"),
        (
            "runtime_canonical_contents",
            "t.application_id=(select application_id from flow_runs where id=$1)",
            "t.id",
        ),
        ("flow_run_recovery_history", "t.flow_run_id=$1", "t.id"),
        (
            "runtime_invocation_context_bindings",
            "t.flow_run_id=$1",
            "t.invocation_span_id",
        ),
        (
            "flow_run_callback_resume_attempts",
            "t.flow_run_id=$1",
            "t.id",
        ),
        ("flow_run_tool_callback_inbox", "t.flow_run_id=$1", "t.id"),
        ("flow_run_resume_claims", "t.flow_run_id=$1", "t.id"),
        ("runtime_events", "t.flow_run_id=$1", "t.id"),
    ] {
        let rows:Value=sqlx::query_scalar(&format!("select coalesce(jsonb_agg(to_jsonb(t) order by {order}),'[]'::jsonb) from {table} t where {predicate}"))
            .bind(f.flow.flow_run_id).fetch_one(f.store.pool()).await.unwrap();
        result.insert(table.into(), rows);
    }
    Value::Object(result)
}

pub(super) async fn assert_apply_refused_unchanged(
    f: &Fixture,
    plan: &ResponsesHistoryMigrationPlan,
) {
    let before = durable_state(f).await;
    assert!(f
        .store
        .apply_responses_history_migration(plan, KEY)
        .await
        .is_err());
    assert_eq!(
        durable_state(f).await,
        before,
        "rejection must leave all owners and audit rows unchanged"
    );
}

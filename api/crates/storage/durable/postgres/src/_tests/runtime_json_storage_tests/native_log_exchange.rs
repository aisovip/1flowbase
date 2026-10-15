use super::*;

#[tokio::test]
async fn native_log_exchange_reads_original_canonical_units_with_scope_isolation_and_no_writes() {
    let (store, run, app, scope) = seed().await;
    let original = json!({"role":"assistant","content":"summary must not replace the original","_source_item":{"type":"function_call","call_id":"real-call","name":"tool","arguments":"long source\0body"}});
    // Deliberately adversarial canonical MD5 identities, all with the same time.
    let units = [
        (
            0_i64,
            "c59f4c66-1268-6f3d-5edc-e03c1bf3a92a",
            "prompt:00000000-0000-0000-0000-000000000001",
            json!({"role":"user","content":"source prompt"}),
        ),
        (
            1,
            "a8ac9109-c1e1-4ee4-e3f9-add15b817a3a",
            "output:call-1",
            original.clone(),
        ),
        (
            2,
            "90025379-0f39-cc88-2760-2f6edbd2a04a",
            "result:tool-1",
            json!({"role":"tool","_source_item":{"type":"function_call_output","call_id":"real-call","output":"source result"}}),
        ),
        (
            3,
            "7ce7b484-03eb-f242-b050-18fd91ad1da6",
            "output:answer",
            json!({"role":"assistant","content":"source answer"}),
        ),
    ];
    for (sequence, id, key, body) in &units {
        // Persist the lossless original in the existing sidecar, as the writer does.
        sqlx::query("insert into application_run_conversation_message_items(id,scope_id,application_id,flow_run_id,display_sequence,source_kind,role,content,native_message,source_item_key,status,projection_version,started_at,can_open_detail,is_current,raw_json_payloads) values($1,$2,$3,$4,$5,'current_run','assistant','display summary','{}',$6,'running',1,'2026-10-07T08:00:00Z',true,true,jsonb_build_object('native_message',$7::text))")
            .bind(Uuid::parse_str(id).unwrap()).bind(scope).bind(app).bind(run).bind(sequence).bind(key).bind(body.to_string()).execute(store.pool()).await.unwrap();
    }
    let count_before: i64 = sqlx::query_scalar(
        "select count(*) from application_run_conversation_message_items where flow_run_id=$1",
    )
    .bind(run)
    .fetch_one(store.pool())
    .await
    .unwrap();
    let first = store
        .read_native_agent_log_messages(scope, app, run)
        .await
        .unwrap();
    assert_eq!(first.len(), 4);
    assert_eq!(first[1]["native_message"], original);
    assert_eq!(
        first
            .iter()
            .map(|unit| unit["display_sequence"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );
    for (unit, (_, id, key, body)) in first.iter().zip(&units) {
        assert_eq!(unit["id"], *id);
        assert_eq!(unit["source_item_key"], *key);
        assert_eq!(unit["native_message"], *body);
        assert_eq!(unit["started_at"], first[0]["started_at"]);
    }
    assert_eq!(
        store
            .read_native_agent_log_messages(scope, app, run)
            .await
            .unwrap(),
        first
    );
    for (foreign_scope, foreign_app, foreign_run) in [
        (Uuid::now_v7(), app, run),
        (scope, Uuid::now_v7(), run),
        (scope, app, Uuid::now_v7()),
    ] {
        assert!(store
            .read_native_agent_log_messages(foreign_scope, foreign_app, foreign_run)
            .await
            .unwrap()
            .is_empty());
    }
    let count_after: i64 = sqlx::query_scalar(
        "select count(*) from application_run_conversation_message_items where flow_run_id=$1",
    )
    .bind(run)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(
        count_before, count_after,
        "exchange reads must not regenerate history"
    );
}

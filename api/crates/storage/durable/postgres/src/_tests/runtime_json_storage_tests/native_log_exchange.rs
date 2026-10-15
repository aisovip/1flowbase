use super::*;

#[tokio::test]
async fn native_log_exchange_reads_original_canonical_units_with_scope_isolation_and_no_writes() {
    let (store, run, app, scope) = seed().await;
    let original = json!({"role":"assistant","content":"summary must not replace the original","_source_item":{"type":"function_call","call_id":"real-call","name":"tool","arguments":"long source\0body"}});
    // Persist a lossless original in the existing sidecar, as the canonical writer does.
    sqlx::query("insert into application_run_conversation_message_items(id,scope_id,application_id,flow_run_id,display_sequence,source_kind,role,content,native_message,source_item_key,status,projection_version,started_at,can_open_detail,is_current,raw_json_payloads) values($1,$2,$3,$4,1,'current_run','assistant','display summary','{}','output:tool:real-call','running',1,now(),true,true,jsonb_build_object('native_message',$5::text))")
        .bind(Uuid::now_v7()).bind(scope).bind(app).bind(run).bind(original.to_string()).execute(store.pool()).await.unwrap();
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
    assert_eq!(first.len(), 1);
    assert_eq!(first[0]["native_message"], original);
    assert_eq!(first[0]["source_item_key"], "output:tool:real-call");
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

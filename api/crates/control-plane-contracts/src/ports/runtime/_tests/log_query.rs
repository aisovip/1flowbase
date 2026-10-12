use super::*;
use domain::{ResourceFilterExpr as F, ResourceFilterOperator as O};
use serde_json::json;
fn predicate(field: &str, operator: O, value: serde_json::Value) -> F {
    F::Field {
        field: field.into(),
        operator,
        value,
    }
}
#[test]
fn validate_rejects_unknown_fields_types_and_nontext_contains_before_empty_scan() {
    let fields = record_trajectory_query_fields();
    for bad in [
        predicate("raw", O::Includes, json!("secret")),
        predicate("request_id", O::Eq, json!("bogus")),
        predicate("sequence", O::Includes, json!("1")),
        predicate("created_at", O::Gt, json!("yesterday")),
        predicate("status", O::In, json!(["succeeded", 17])),
    ] {
        let error =
            validate_log_query_filter(&F::Any(vec![F::All(vec![]), bad]), &fields).unwrap_err();
        assert!(matches!(
            error.downcast_ref::<crate::ControlPlaneContractError>(),
            Some(crate::ControlPlaneContractError::InvalidInput(_))
        ));
    }
}
#[test]
fn summary_filter_preserves_nullable_fields_and_all_any_semantics() {
    let fields = record_trajectory_query_fields();
    let record = json!({"category":"tool","name":"Search","namespace":null,"preview":"Short summary","status":"succeeded","sequence":19});
    let filter = F::All(vec![
        predicate("namespace", O::Eq, json!(null)),
        F::Any(vec![
            predicate("name", O::Includes, json!("SEARCH")),
            predicate("status", O::Eq, json!("failed")),
        ]),
        predicate("sequence", O::Gt, json!(18)),
    ]);
    validate_log_query_filter(&filter, &fields).unwrap();
    assert!(log_query_filter_matches(&filter, &record, &fields));
    assert!(!log_query_filter_matches(
        &predicate("namespace", O::Ne, json!("foo")),
        &record,
        &fields
    ));
    assert!(!log_query_filter_matches(&F::Any(vec![]), &record, &fields));
    assert!(log_query_filter_matches(&F::All(vec![]), &record, &fields));
    assert!(!log_query_filter_matches(
        &predicate("preview", O::Includes, json!("body-only keyword")),
        &record,
        &fields
    ));
}
#[test]
fn comparisons_use_actual_uuid_and_timestamp_types() {
    let fields = record_trajectory_query_fields();
    let record = json!({"request_id":"ABCDEFAB-CDEF-ABCD-EFAB-CDEFABCDEFAB","created_at":"2026-10-09T12:00:00+08:00"});
    assert!(log_query_filter_matches(
        &predicate(
            "request_id",
            O::Eq,
            json!("abcdefab-cdef-abcd-efab-cdefabcdefab")
        ),
        &record,
        &fields
    ));
    assert!(log_query_filter_matches(
        &predicate("created_at", O::Eq, json!("2026-10-09T04:00:00Z")),
        &record,
        &fields
    ));
}
fn trajectory_query() -> RecordClientTrajectoryQuery {
    RecordClientTrajectoryQuery {
        filter: F::All(vec![]),
        cursor: None,
        limit: 10,
        keyword: Some("answer".into()),
        search_sections: vec![],
    }
}
#[test]
fn search_defaults_to_result_and_never_accepts_raw_or_unknown_sections() {
    let mut query = trajectory_query();
    assert_eq!(log_query_search_sections(&query).unwrap(), vec!["result"]);
    for section in ["raw", "unknown"] {
        query.search_sections = vec![section.into()];
        assert!(log_query_search_sections(&query).is_err());
    }
    query.search_sections = vec!["overview".into(), "parameters".into(), "overview".into()];
    assert_eq!(
        log_query_search_sections(&query).unwrap(),
        vec!["overview", "parameters"]
    );
    query.keyword = Some(" ".into());
    assert!(log_query_search_sections(&query).is_err());
}
#[test]
fn snippets_restore_nul_and_multibyte_offsets_without_global_lowercase_offsets() {
    assert_eq!(
        log_query_snippet("before\0答案之后", "答案"),
        Some("before\0答案之后".into())
    );
    assert_eq!(
        log_query_snippet("İ ANSWER 结束", "answer"),
        Some("İ ANSWER 结束".into())
    );
    assert_eq!(log_query_snippet("other body", "secret"), None);
    let text = format!("{}答案{}", "前".repeat(200), "后".repeat(200));
    let snippet = log_query_snippet(&text, "答案").unwrap();
    assert!(snippet.contains("答案"));
    assert!(snippet.chars().count() < 170);
}
#[test]
fn cursors_bind_scope_visibility_filter_sort_and_search_but_not_page_size() {
    let scope = Uuid::from_u128(101);
    let app = Uuid::from_u128(102);
    let other = Uuid::from_u128(103);
    let mut query = ApplicationLogRecordsQuery {
        filter: F::All(vec![]),
        sort_field: "started_at".into(),
        descending: true,
        cursor: None,
        limit: 10,
    };
    let binding = application_log_query_fingerprint(scope, &[app, other], &query);
    query.limit = 20;
    assert_eq!(
        binding,
        application_log_query_fingerprint(scope, &[other, app, app], &query)
    );
    for changed in [
        application_log_query_fingerprint(Uuid::from_u128(104), &[app, other], &query),
        application_log_query_fingerprint(scope, &[app], &query),
    ] {
        assert!(validate_log_query_cursor_binding(1, &binding, &changed).is_err());
    }
    query.filter = predicate("status", O::Eq, json!("failed"));
    assert_ne!(
        binding,
        application_log_query_fingerprint(scope, &[app, other], &query)
    );
    query.filter = F::All(vec![]);
    query.descending = false;
    assert_ne!(
        binding,
        application_log_query_fingerprint(scope, &[app, other], &query)
    );
    assert!(validate_log_query_cursor_binding(2, &binding, &binding).is_err());
    let mut query = trajectory_query();
    let record = Uuid::from_u128(105);
    let sections = log_query_search_sections(&query).unwrap();
    let binding = record_trajectory_query_fingerprint(app, record, &query, &sections);
    query.keyword = Some("different".into());
    assert_ne!(
        binding,
        record_trajectory_query_fingerprint(app, record, &query, &sections)
    );
    assert_ne!(
        binding,
        record_trajectory_query_fingerprint(other, record, &trajectory_query(), &sections)
    );
    assert_ne!(
        binding,
        record_trajectory_query_fingerprint(
            app,
            Uuid::from_u128(106),
            &trajectory_query(),
            &sections
        )
    );
}

#[test]
fn nul_predicate_is_supported_by_restored_trajectory_and_rejected_for_task_text() {
    let filter = predicate("preview", O::Includes, json!("\u{0}"));
    validate_log_query_filter(&filter, &record_trajectory_query_fields()).unwrap();
    assert!(log_query_filter_matches(
        &filter,
        &json!({"preview":"source\u{0}identity"}),
        &record_trajectory_query_fields()
    ));
    assert!(validate_application_log_query_filter(&predicate(
        "user_input",
        O::Includes,
        json!("\u{0}")
    ))
    .is_err());
}

#[test]
fn new_page_limit_has_no_guessed_business_ceiling() {
    assert_eq!(log_query_page_lookahead(100_001).unwrap(), 100_002);
    assert_eq!(log_query_page_lookahead(1).unwrap(), 2);
    for invalid in [0, -1, i64::MAX] {
        assert!(log_query_page_lookahead(invalid).is_err());
    }
}
#[test]
fn filter_contains_uses_sql_wildcards_but_keyword_search_is_literal() {
    assert!(log_query_text_pattern_matches("aXYZb", "a%b"));
    assert!(log_query_text_pattern_matches("aXb", "a_b"));
    assert!(log_query_text_pattern_matches("a_b", r"a\_b"));
    assert!(!log_query_text_pattern_matches("aXb", r"a\_b"));
    assert!(log_query_text_pattern_matches("literal%", r"literal\"));
    assert!(!log_query_text_pattern_matches("literalx", r"literal\"));
    assert_eq!(log_query_snippet("aXb", "a_b"), None);
    assert_eq!(log_query_snippet("a_b", "a_b"), Some("a_b".into()));
}

#[test]
fn numeric_sequence_comparison_preserves_integer_precision_and_fractional_order() {
    let fields = record_trajectory_query_fields();
    let sequence = 9_007_199_254_740_993_i64;
    let record = json!({"sequence":sequence});
    assert!(!log_query_filter_matches(
        &predicate("sequence", O::Eq, json!(sequence - 1)),
        &record,
        &fields
    ));
    assert!(log_query_filter_matches(
        &predicate("sequence", O::Gt, json!(sequence - 1)),
        &record,
        &fields
    ));
    assert!(!log_query_filter_matches(
        &predicate("sequence", O::In, json!([sequence - 1])),
        &record,
        &fields
    ));
    assert!(log_query_filter_matches(
        &predicate("sequence", O::In, json!([sequence])),
        &record,
        &fields
    ));
    assert!(log_query_filter_matches(
        &predicate("sequence", O::Lt, json!(9_223_372_036_854_775_808_f64)),
        &json!({"sequence":i64::MAX}),
        &fields
    ));
    assert!(log_query_filter_matches(
        &predicate("sequence", O::Gt, json!(-1.25)),
        &json!({"sequence":-1}),
        &fields
    ));
    assert!(log_query_filter_matches(
        &predicate("sequence", O::Lt, json!(0.5)),
        &json!({"sequence":0}),
        &fields
    ));
    assert!(log_query_filter_matches(
        &predicate("sequence", O::Eq, json!(1.0)),
        &json!({"sequence":1}),
        &fields
    ));
    assert!(log_query_filter_matches(
        &predicate("sequence", O::Lt, json!(18_446_744_073_709_551_616_f64)),
        &json!({"sequence":u64::MAX}),
        &fields
    ));
}

#[test]
fn record_trace_contract_is_explicit_metadata_with_record_locator() {
    let record = Uuid::now_v7();
    let child = Uuid::now_v7();
    let value = serde_json::to_value(ApplicationLogRecordTraceChildren {
        items: vec![ApplicationLogRecordTraceNode {
            trace_node_id: child,
            record_id: child,
            parent_trace_node_id: Some(record),
            node_kind: "child_task".into(),
            node_alias: "delegated task".into(),
            status: "running".into(),
            started_at: "2026-10-12T00:00:00Z".into(),
            finished_at: None,
            duration_ms: None,
            metrics_payload: json!({"total_tokens":42}),
            has_children: false,
            child_count: 0,
            has_content: true,
            source_client: Some("codex".into()),
            source_session_id: Some("child-session".into()),
            source_task_id: Some("child-task".into()),
        }],
        page_info: ApplicationLogRecordTracePageInfo {
            has_more: false,
            next_cursor: None,
            page_size: 107,
        },
    })
    .unwrap();
    assert_eq!(value["items"][0]["record_id"], json!(child));
    assert_eq!(value["items"][0]["parent_trace_node_id"], json!(record));
    assert_eq!(value["page_info"]["page_size"], 107);
    for field in ["flow_run_id", "messages", "raw", "content"] {
        assert!(value["items"][0].get(field).is_none());
    }
    let roundtrip: ApplicationLogRecordTraceChildren = serde_json::from_value(value).unwrap();
    assert_eq!(roundtrip.items[0].trace_node_id, child);
}

#[test]
fn conversation_trajectory_provenance_filter_and_fingerprint_contract() {
    let fields = conversation_trajectory_query_fields();
    for (name, kind) in [
        ("is_subagent", LogQueryValueType::Boolean),
        ("record_id", LogQueryValueType::Uuid),
        ("parent_task_run_id", LogQueryValueType::Uuid),
        ("source_session_id", LogQueryValueType::String),
    ] {
        assert!(fields
            .iter()
            .any(|field| field.field == name && field.value_type == kind));
    }
    let app = Uuid::from_u128(1);
    let scope = Uuid::from_u128(2);
    let conversation = Uuid::from_u128(3);
    let mut query = ConversationClientTrajectoryQuery {
        filter: domain::ResourceFilterExpr::All(vec![]),
        cursor: None,
        limit: 1,
        keyword: None,
        search_sections: vec![],
        focus_record_id: None,
    };
    let fingerprint = |query: &ConversationClientTrajectoryQuery, scope, source| {
        conversation_trajectory_query_fingerprint(
            app,
            scope,
            conversation,
            source,
            "codex",
            query,
            &[],
        )
    };
    let initial = fingerprint(&query, scope, "collector");
    query.limit = 500;
    query.cursor = Some("opaque".into());
    assert_eq!(initial, fingerprint(&query, scope, "collector"));
    assert_ne!(
        initial,
        fingerprint(&query, Uuid::from_u128(4), "collector")
    );
    assert_ne!(initial, fingerprint(&query, scope, "foreign"));
    query.focus_record_id = Some(Uuid::from_u128(5));
    assert_ne!(initial, fingerprint(&query, scope, "collector"));
}

use super::*;

#[test]
fn exchange_managed_projection_excludes_credentials_destinations_and_facts() {
    let id = Uuid::now_v7();
    let secret = "PRIVATE_SOURCE_VALUE";
    let inputs = [
        ExchangeInput::OtlpImport {
            application_id: id,
            body: OtlpImportBody {
                otlp: json!({"raw":secret}),
            },
        },
        ExchangeInput::LangfuseImport {
            application_id: id,
            body: LangfuseImportBody {
                trace_ids: vec![secret.into()],
                base_url: secret.into(),
                public_key: secret.into(),
                secret_key: secret.into(),
            },
        },
        ExchangeInput::LangfuseExport {
            application_id: id,
            body: LangfuseExportBody {
                record_ids: vec![id],
                base_url: secret.into(),
                public_key: secret.into(),
                secret_key: secret.into(),
            },
        },
    ];
    for input in inputs {
        let projection = input.project_for_managed_hook().unwrap();
        assert_eq!(projection.as_object().unwrap().len(), 3);
        assert!(!projection.to_string().contains(secret));
    }
    for output in [
        ExchangeOutput::Otlp(json!({"raw":secret})),
        ExchangeOutput::Exported(LangfuseExportReceipt {
            exported_records: 1,
            exported_spans: 2,
            trace_ids: vec![secret.into()],
        }),
    ] {
        assert!(!output
            .project_for_managed_hook()
            .unwrap()
            .to_string()
            .contains(secret));
    }
}

#[test]
fn exchange_has_four_independent_operations_and_strict_bodies() {
    let assembly = route_assembly();
    assert_eq!(assembly.bindings().len(), 4);
    for (binding, operation) in assembly.bindings().iter().zip(OPERATIONS) {
        assert_eq!(binding.route.method, "POST");
        assert_eq!(
            binding.ownership,
            access_control::ConsoleRouteOwnership::ConsoleOperation((*operation).into())
        );
    }
    assert!(serde_json::from_value::<OtlpImportBody>(
        json!({"otlp":{},"secret_key":"unregistered"})
    )
    .is_err());
    assert!(serde_json::from_value::<LangfuseImportBody>(json!({"trace_ids":[],"base_url":"http://localhost","public_key":"pk","secret_key":"sk","api_key_id":Uuid::nil()})).is_err());
}

#[tokio::test]
async fn partial_import_error_reports_safe_committed_counts() {
    use axum::response::IntoResponse;
    let response = ApiError(
        control_plane::agent_logs::AgentLogsPartialImportError {
            accepted_events: 2,
            duplicate_events: 1,
            committed_records: 1,
        }
        .into(),
    )
    .into_response();
    assert_eq!(response.status(), axum::http::StatusCode::CONFLICT);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["code"], "agent_logs_partial_import");
    assert_eq!(
        value["details"],
        json!({"accepted_events":2,"duplicate_events":1,"committed_records":1})
    );
}

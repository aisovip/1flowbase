//! Managed hooks receive counts only; source bodies and remote destinations stay private.
use super::*;
use crate::extension_bus::managed_projection as mp;
impl InterfaceContract for ExchangeInput {
    const CONTRACT_ID: &'static str = "console-application-log-exchange-input";
    const CONTRACT_VERSION: &'static str = "1";
    fn managed_projection_schema() -> Option<Value> {
        Some(mp::object_schema(&[
            ("application_id", mp::text_schema()),
            ("action", mp::text_schema()),
            (
                "selected_items",
                mp::union_schema(vec![mp::count_schema(), json!({"type":"null"})]),
            ),
        ]))
    }
    fn project_for_managed_hook(&self) -> Option<Value> {
        let count = match self {
            Self::OtlpExport { body, .. } => json!(body.record_ids.len()),
            Self::OtlpImport { .. } => Value::Null,
            Self::LangfuseExport { body, .. } => json!(body.record_ids.len()),
            Self::LangfuseImport { body, .. } => json!(body.trace_ids.len()),
        };
        Some(
            json!({"application_id":self.identity().0,"action":self.action(),"selected_items":count}),
        )
    }
}
impl InterfaceContract for ExchangeOutput {
    const CONTRACT_ID: &'static str = "console-application-log-exchange-output";
    const CONTRACT_VERSION: &'static str = "1";
    fn managed_projection_schema() -> Option<Value> {
        Some(mp::object_schema(&[
            ("action", mp::text_schema()),
            (
                "records",
                mp::union_schema(vec![mp::count_schema(), json!({"type":"null"})]),
            ),
            ("events", mp::count_schema()),
            ("duplicates", mp::count_schema()),
        ]))
    }
    fn project_for_managed_hook(&self) -> Option<Value> {
        Some(match self {
            Self::Otlp(value) => {
                let events = value
                    .get("resourceSpans")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|resource| resource.get("scopeSpans").and_then(Value::as_array))
                    .flatten()
                    .filter_map(|scope| scope.get("spans").and_then(Value::as_array))
                    .map(Vec::len)
                    .sum::<usize>();
                json!({"action":"otlp_export","records":null,"events":events,"duplicates":0})
            }
            Self::Imported(receipt) => {
                json!({"action":"import","records":receipt.record_ids.len(),"events":receipt.accepted_events,"duplicates":receipt.duplicate_events})
            }
            Self::Exported(receipt) => {
                json!({"action":"langfuse_export","records":receipt.exported_records,"events":receipt.exported_spans,"duplicates":0})
            }
        })
    }
}

use super::*;

pub(super) struct NativeGroup {
    source: SourceLocator,
    body: Value,
    output: BTreeMap<u64, Value>,
    received: bool,
    failure: Option<String>,
}

impl NativeGroup {
    pub(super) fn close(mut self) -> SourceGroup {
        if !self.received && self.failure.is_none() {
            self.failure = Some("history_source_native_completion_missing".into());
        }
        SourceGroup {
            source: self.source,
            body: self.body,
            output: self.output.into_values().collect(),
            expected_public: None,
            failure: self.failure,
        }
    }

    fn same_attempt(&self, payload: &Value, required: bool) -> bool {
        let invocation_matches = match payload.get("invocation_id") {
            None => !required,
            Some(value) => value
                .as_str()
                .is_some_and(|id| self.source.invocation_id.as_deref() == Some(id)),
        };
        let attempt_matches = match payload.get("provider_attempt_index") {
            None => !required,
            Some(value) => value
                .as_u64()
                .is_some_and(|attempt| self.source.provider_attempt_index == Some(attempt)),
        };
        invocation_matches && attempt_matches
    }
}

impl HistorySourceGraph {
    pub(super) fn feed_native(
        &mut self,
        flow_run_id: Uuid,
        node_run_id: Option<Uuid>,
        event_id: Uuid,
        sequence: i64,
        event_type: &str,
        payload: &Value,
    ) -> Result<()> {
        let prepared = event_type == "provider_semantic_step"
            && payload["kind"] == "model_call"
            && payload["direction"] == "prepared";
        let received = event_type == "provider_semantic_step"
            && payload["kind"] == "model_reply"
            && payload["direction"] == "received";
        let failed = event_type == "provider_semantic_step" && payload["kind"] == "error";
        let done = event_type == "provider_output_item_done";
        if !(prepared || received || failed || done) {
            return Ok(());
        }
        let payload_node = payload
            .get("node_run_id")
            .filter(|value| !value.is_null())
            .map(|value| serde_json::from_value::<Uuid>(value.clone()))
            .transpose()?;
        if let (Some(node), Some(payload_node)) = (node_run_id, payload_node) {
            ensure!(
                node == payload_node,
                "history_source_node_identity_mismatch"
            );
        }
        if let Some(flow) = payload.get("flow_run_id").filter(|value| !value.is_null()) {
            ensure!(
                serde_json::from_value::<Uuid>(flow.clone())? == flow_run_id,
                "history_source_flow_identity_mismatch"
            );
        }
        let node_run_id = node_run_id
            .or(payload_node)
            .ok_or_else(|| anyhow::anyhow!("history_source_node_identity_missing"))?;
        let group_key = (flow_run_id, node_run_id);
        if let Some(previous) = self.native_sequence.get(&group_key) {
            ensure!(
                sequence > *previous,
                "history_source_sequence_not_increasing"
            );
        }
        self.native_sequence.insert(group_key, sequence);
        if prepared {
            if let Some(previous) = self.active_native.remove(&group_key) {
                self.pending.push(previous.close());
            }
            let source = SourceLocator {
                namespace: SourceNamespace::Native,
                flow_run_id,
                node_run_id: Some(node_run_id),
                source_id: event_id,
                sequence: Some(sequence),
                completion_source_id: None,
                invocation_id: payload
                    .get("invocation_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                provider_attempt_index: payload
                    .get("provider_attempt_index")
                    .and_then(Value::as_u64),
                response_id: None,
            };
            let body = native_body(payload).and_then(|body| {
                body.get("native_request")
                    .and_then(|request| request.get("wire_body"))
                    .filter(|body| body.is_object())
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("history_source_native_wire_body_missing"))
            });
            let failure = if source.invocation_id.as_deref().is_none_or(str::is_empty)
                || source.provider_attempt_index.is_none()
            {
                Some("history_source_native_attempt_missing".into())
            } else if payload["status"] != "recorded" {
                Some("history_source_native_prepared_incomplete".into())
            } else {
                body.as_ref().err().map(ToString::to_string)
            };
            self.active_native.insert(
                group_key,
                NativeGroup {
                    source,
                    body: body.unwrap_or(Value::Null),
                    output: BTreeMap::new(),
                    received: false,
                    failure,
                },
            );
            return Ok(());
        }
        let Some(group) = self.active_native.get_mut(&group_key) else {
            self.failed.push(UnresolvedSource {
                source: SourceLocator {
                    namespace: SourceNamespace::Native,
                    flow_run_id,
                    node_run_id: Some(node_run_id),
                    source_id: event_id,
                    sequence: Some(sequence),
                    completion_source_id: None,
                    invocation_id: None,
                    provider_attempt_index: None,
                    response_id: None,
                },
                reason: "history_source_native_prepared_missing".into(),
            });
            return Ok(());
        };
        if !group.same_attempt(payload, received || failed) {
            group.failure = Some("history_source_native_attempt_mismatch".into());
            return Ok(());
        }
        if failed {
            group.failure = Some("history_source_native_invocation_failed".into());
        } else if done {
            let Some(index) = payload.get("output_index").and_then(Value::as_u64) else {
                group.failure = Some("history_source_native_output_index_invalid".into());
                return Ok(());
            };
            let Some(item) = payload.get("item").filter(|value| value.is_object()) else {
                group.failure = Some("history_source_native_output_item_invalid".into());
                return Ok(());
            };
            match group.output.get(&index) {
                Some(previous) if previous != item => {
                    group.failure = Some("history_source_native_output_index_conflict".into());
                }
                Some(_) => {}
                None => {
                    group.output.insert(index, item.clone());
                }
            }
        } else if received {
            if group.received {
                group.failure = Some("history_source_native_completion_conflict".into());
                return Ok(());
            }
            group.received = true;
            group.source.completion_source_id = Some(event_id);
            let body = match native_body(payload) {
                Ok(body) => body,
                Err(error) => {
                    group.failure = Some(error.to_string());
                    return Ok(());
                }
            };
            if payload["status"] != "recorded"
                || !matches!(
                    body["result"]["finish_reason"].as_str(),
                    Some("stop" | "tool_call" | "mcp_call")
                )
            {
                group.failure = Some("history_source_native_completion_unsuccessful".into());
            }
            match body["result"].get("response_id") {
                Some(Value::String(id)) if !id.is_empty() => {
                    group.source.response_id = Some(id.clone())
                }
                None | Some(Value::Null) => {}
                _ => group.failure = Some("history_source_native_response_id_invalid".into()),
            }
            // Observation output_items intentionally excludes split tool records.
            // Only indexed provider_output_item_done facts populate group.output.
        }
        Ok(())
    }
}

fn native_body(payload: &Value) -> Result<Value> {
    let body = payload
        .get("body")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("history_source_native_original_body_missing"))?;
    Ok(serde_json::from_str(body)?)
}

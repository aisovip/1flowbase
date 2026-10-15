use super::*;

#[async_trait]
impl ResponsesHistoryMigrationReadRepository for PgControlPlaneStore {
    async fn list_responses_history_flows(
        &self,
        input: &ResponsesHistoryFlowPageInput,
    ) -> Result<ResponsesHistoryFlowPage> {
        anyhow::ensure!(input.limit > 0, "responses_history_page_limit_invalid");
        let mut ids = sqlx::query_scalar::<_, Uuid>(
            "select id from flow_runs where scope_id=$1 and application_id=$2 and ($3::uuid is null or id>$3) order by id limit $4",
        )
        .bind(input.scope_id)
        .bind(input.application_id)
        .bind(input.after_flow_run_id)
        .bind(i64::from(input.limit) + 1)
        .fetch_all(self.pool())
        .await?;
        let has_more = ids.len() > input.limit as usize;
        ids.truncate(input.limit as usize);
        let next_after_flow_run_id = has_more.then(|| *ids.last().expect("nonempty bounded page"));
        Ok(ResponsesHistoryFlowPage {
            flows: ids
                .into_iter()
                .map(|flow_run_id| ResponsesHistoryFlowKey {
                    scope_id: input.scope_id,
                    application_id: input.application_id,
                    flow_run_id,
                })
                .collect(),
            next_after_flow_run_id,
        })
    }

    async fn list_responses_history_native_sources(
        &self,
        input: &ResponsesHistoryNativeSourcePageInput,
    ) -> Result<ResponsesHistoryNativeSourcePage> {
        anyhow::ensure!(input.limit > 0, "responses_history_page_limit_invalid");
        let mut rows = sqlx::query(
            r#"select e.id,e.node_run_id,e.sequence,e.event_type,e.scope_id,
                      (n.flow_run_id=f.id and n.scope_id=f.scope_id) as node_valid,
                      runtime_event_original_payload(e.payload,e.raw_json_payloads,e.flow_run_id) as payload
                 from runtime_events e
                 join flow_runs f on f.id=e.flow_run_id
                 left join node_runs n on n.id=e.node_run_id
                where f.id=$1 and f.scope_id=$2 and f.application_id=$3
                  and ($4::bigint is null or e.sequence>$4)
                  and (e.event_type='provider_output_item_done' or
                      (e.event_type='provider_semantic_step' and e.payload->>'source'='ai_native'
                       and e.payload->>'direction' in ('prepared','received')))
                order by e.sequence limit $5"#,
        )
        .bind(input.flow.flow_run_id)
        .bind(input.flow.scope_id)
        .bind(input.flow.application_id)
        .bind(input.after_sequence)
        .bind(i64::from(input.limit) + 1)
        .fetch_all(self.pool())
        .await?;
        let has_more = rows.len() > input.limit as usize;
        rows.truncate(input.limit as usize);
        let sources = rows
            .into_iter()
            .map(|row| {
                anyhow::ensure!(
                    row.try_get::<Uuid, _>("scope_id")? == input.flow.scope_id
                        && (row.try_get::<Option<Uuid>, _>("node_run_id")?.is_none()
                            || row.try_get::<Option<bool>, _>("node_valid")? == Some(true)),
                    "responses_history_native_source_owner_mismatch"
                );
                Ok(ResponsesHistoryNativeSource {
                    id: row.try_get("id")?,
                    node_run_id: row.try_get("node_run_id")?,
                    sequence: row.try_get("sequence")?,
                    event_type: row.try_get("event_type")?,
                    payload: row.try_get("payload")?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(ResponsesHistoryNativeSourcePage {
            next_after_sequence: has_more
                .then(|| sources.last().expect("nonempty bounded page").sequence),
            sources,
        })
    }
}

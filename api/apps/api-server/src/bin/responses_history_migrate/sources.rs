use anyhow::{ensure, Result};
use control_plane::responses_history_migration::{FlowInspection, HistorySourceGraph};
use control_plane_contracts::ports::{
    responses_history_migration::{
        ResponsesHistoryFlowKey, ResponsesHistoryMigrationReadRepository,
        ResponsesHistoryNativeSourcePageInput,
    },
    OrchestrationRuntimeRepository,
};
use storage_durable_postgres::PgControlPlaneStore;

pub async fn feed(
    repository: &PgControlPlaneStore,
    graph: &mut HistorySourceGraph,
    flow: &ResponsesHistoryFlowKey,
    inspection: FlowInspection,
    page_size: u32,
) -> Result<()> {
    let mut after_sequence = None;
    loop {
        let page = repository
            .list_responses_history_native_sources(&ResponsesHistoryNativeSourcePageInput {
                flow: *flow,
                after_sequence,
                limit: page_size,
            })
            .await?;
        for source in page.sources {
            graph.push_native(
                flow.flow_run_id,
                source.node_run_id,
                source.id,
                source.sequence,
                &source.event_type,
                &source.payload,
            )?;
        }
        match page.next_after_sequence {
            Some(next) => {
                ensure!(
                    after_sequence.is_none_or(|old| next > old),
                    "native source cursor did not advance"
                );
                after_sequence = Some(next);
            }
            None => break,
        }
    }
    graph.finish_native_flow(flow.flow_run_id)?;
    let has_request_input_targets = inspection
        .request_input_histories
        .iter()
        .any(|proof| proof["version"] != 3);
    if inspection.public_rounds.is_empty() && !has_request_input_targets {
        return Ok(());
    }
    let mut cursor = None;
    loop {
        let page = repository
            .client_trajectory_page(flow.flow_run_id, None, cursor, i64::from(page_size))
            .await?;
        for step in page.items {
            if step.category != "request" || !step.available_sections.iter().any(|s| s == "raw") {
                continue;
            }
            let mut section_cursor = None;
            loop {
                let section = repository
                    .client_trajectory_section(
                        flow.flow_run_id,
                        None,
                        step.id,
                        "raw",
                        section_cursor,
                        i64::from(page_size),
                    )
                    .await?;
                let Some(section) = section else {
                    break;
                };
                ensure!(
                    section.step_id == step.id && section.request_id == step.request_id,
                    "original request section ownership mismatch"
                );
                for mut item in section.items {
                    if item.value["frame_kind"] != "request" {
                        continue;
                    }
                    item.value["sequence"] = serde_json::json!(item.sequence);
                    if has_request_input_targets {
                        graph.push_public_request_input(
                            flow.flow_run_id,
                            section.request_id,
                            item.sequence,
                            &item.value,
                        )?;
                    }
                    // Same-flow requests are candidates, never assumed associations.
                    // The graph publishes prefixes only after the complete persisted
                    // round proof matches. Failed candidates remain in its report.
                    for round in &inspection.public_rounds {
                        graph.push_public(flow.flow_run_id, step.id, &item.value, round)?;
                    }
                }
                match section.next_cursor {
                    Some(next) => {
                        ensure!(
                            section_cursor != Some(next),
                            "raw source cursor did not advance"
                        );
                        section_cursor = Some(next);
                    }
                    None => break,
                }
            }
        }
        match page.next_cursor {
            Some(next) => {
                ensure!(cursor != Some(next), "client source cursor did not advance");
                cursor = Some(next);
            }
            None => break,
        }
    }
    Ok(())
}

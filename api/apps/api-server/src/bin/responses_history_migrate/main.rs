//! Explicit offline maintenance executable; never linked into server startup.
//! Ordinary builds do not enable frozen legacy digest code.
use anyhow::{ensure, Context, Result};
use control_plane::responses_history_migration::{
    apply_verified_flow, build_plan, inspect_flow, HistorySourceGraph, ScopedHistoryTarget,
};
use control_plane_contracts::ports::responses_history_migration::*;
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use std::collections::BTreeMap;
use storage_durable_postgres::PgControlPlaneStore;

mod options;
mod private_files;
mod sources;

#[tokio::main]
async fn main() -> Result<()> {
    let options = options::Options::parse(std::env::args().skip(1))?;
    let url = std::env::var("API_DATABASE_URL").context("API_DATABASE_URL required")?;
    let key = std::env::var("API_PROVIDER_SECRET_MASTER_KEY")
        .context("API_PROVIDER_SECRET_MASTER_KEY required; no inferred key")?;
    let pool = PgPoolOptions::new()
        .connect(&url)
        .await
        .context("maintenance database connection failed")?;
    let database: String = sqlx::query_scalar("select current_database()")
        .fetch_one(&pool)
        .await?;
    ensure!(
        database == options.database && database != "1flowbase_gateway",
        "database identity mismatch or protected database"
    );
    private_files::create_directory(&options.output)?;
    private_files::write_json(
        &options.output.join("identity.json"),
        &json!({
            "database": database, "scope_id": options.scope, "application_id": options.application,
            "apply": options.apply, "process_id": std::process::id(),
            "excluded_flows": options.excluded_flows,
        }),
    )?;
    let repository = PgControlPlaneStore::new(pool.clone());
    let mut flows = Vec::new();
    let mut after_flow_run_id = None;
    loop {
        let page = repository
            .list_responses_history_flows(&ResponsesHistoryFlowPageInput {
                scope_id: options.scope,
                application_id: options.application,
                after_flow_run_id,
                limit: options.page_size,
            })
            .await?;
        flows.extend(page.flows);
        match page.next_after_flow_run_id {
            Some(next) => {
                ensure!(
                    after_flow_run_id.is_none_or(|old| next > old),
                    "flow cursor did not advance"
                );
                after_flow_run_id = Some(next);
            }
            None => break,
        }
    }
    options.validate_exclusions(flows.iter().map(|flow| flow.flow_run_id))?;
    let all_flows = flows;
    let flows: Vec<_> = all_flows
        .iter()
        .filter(|flow| !options.excluded_flows.contains(&flow.flow_run_id))
        .copied()
        .collect();
    let mut proofs = BTreeMap::<(uuid::Uuid, u64, u64, String), ScopedHistoryTarget>::new();
    let mut full_targets = Vec::new();
    let mut input_targets = Vec::new();
    let mut versions = BTreeMap::<u64, usize>::new();
    for flow in &flows {
        let snapshot = repository
            .load_responses_history_flow(flow, &key)
            .await
            .with_context(|| format!("inventory load flow {}", flow.flow_run_id))?;
        let inspection = inspect_flow(&snapshot)
            .with_context(|| format!("inventory inspect flow {}", flow.flow_run_id))?;
        full_targets.extend(
            inspection
                .full_source_required_proofs
                .into_iter()
                .map(|proof| ScopedHistoryTarget {
                    flow_run_id: flow.flow_run_id,
                    proof,
                }),
        );
        input_targets.extend(inspection.request_input_histories.into_iter().map(|proof| {
            ScopedHistoryTarget {
                flow_run_id: flow.flow_run_id,
                proof,
            }
        }));
        for proof in inspection.proofs {
            *versions
                .entry(
                    proof["version"]
                        .as_u64()
                        .context("invalid history version")?,
                )
                .or_default() += 1;
            proofs.insert(
                (
                    flow.flow_run_id,
                    proof["version"]
                        .as_u64()
                        .context("invalid history version")?,
                    proof["item_count"]
                        .as_u64()
                        .context("invalid history count")?,
                    proof["digest"]
                        .as_str()
                        .context("invalid history digest")?
                        .to_owned(),
                ),
                ScopedHistoryTarget {
                    flow_run_id: flow.flow_run_id,
                    proof,
                },
            );
        }
    }
    private_files::write_json(
        &options.output.join("inventory.json"),
        &json!({
            "flows": flows.len(), "enumerated_flows": all_flows.len(),
            "excluded_flows": options.excluded_flows,
            "versions": versions, "distinct_proofs": proofs.len(),
        }),
    )?;
    let mut graph = HistorySourceGraph::with_request_input_targets(&full_targets, &input_targets)?;
    // Excluded records remain read-only source candidates for legitimate chains.
    for flow in all_flows
        .iter()
        .filter(|_| versions.keys().any(|version| *version != 3))
    {
        let snapshot = repository
            .load_responses_history_flow(flow, &key)
            .await
            .with_context(|| format!("source load flow {}", flow.flow_run_id))?;
        let inspection = inspect_flow(&snapshot)
            .with_context(|| format!("source inspect flow {}", flow.flow_run_id))?;
        drop(snapshot);
        sources::feed(&repository, &mut graph, flow, inspection, options.page_size)
            .await
            .with_context(|| format!("source feed flow {}", flow.flow_run_id))?;
    }
    let resolution = graph.resolve()?;
    private_files::write_json(
        &options.output.join("source-verification.json"),
        &resolution,
    )?;
    ensure!(
        resolution.unresolved_targets.is_empty(),
        "unverified history proofs remain; no migration writes started"
    );
    // Prepare the complete application before any writes. Plans are private review
    // evidence, never trusted input. Apply below reloads and rebuilds each plan.
    let mut planned = Vec::new();
    for flow in &flows {
        let snapshot = repository
            .load_responses_history_flow(flow, &key)
            .await
            .with_context(|| format!("prepare load flow {}", flow.flow_run_id))?;
        let plan = build_plan(&snapshot, &resolution.verified)
            .with_context(|| format!("prepare plan flow {}", flow.flow_run_id))?;
        if empty(&plan) {
            continue;
        }
        repository
            .preview_responses_history_migration(&plan, &key)
            .await
            .with_context(|| format!("prepare preview flow {}", flow.flow_run_id))?;
        private_files::write_json(
            &options
                .output
                .join(format!("{}.plan.json", flow.flow_run_id)),
            &plan,
        )?;
        planned.push(*flow);
    }
    println!(
        "verified flows={} migration_flows={} source_candidates_unresolved={} mode={}",
        flows.len(),
        planned.len(),
        resolution.unresolved_sources.len(),
        if options.apply { "apply" } else { "dry-run" }
    );
    if !options.apply {
        private_files::write_json(
            &options.output.join("complete.json"),
            &json!({"mode":"dry-run", "migration_flows":planned.len(), "excluded_flows":options.excluded_flows}),
        )?;
        return Ok(());
    }
    if !planned.is_empty() {
        private_files::backup_database(&options.output, &url)?;
    }
    let mut receipts: Vec<ResponsesHistoryMigrationReceipt> = Vec::new();
    let applied: Result<()> = async {
        for flow in &planned {
            let snapshot = repository
                .load_responses_history_flow(flow, &key)
                .await
                .with_context(|| format!("apply load flow {}", flow.flow_run_id))?;
            let plan = build_plan(&snapshot, &resolution.verified)
                .with_context(|| format!("apply plan flow {}", flow.flow_run_id))?;
            // fsync before commit. A process crash in the receipt window is explicit
            // in the journal and recoverable from before.dump with writers stopped.
            private_files::write_json(
                &options
                    .output
                    .join(format!("{}.intent.json", flow.flow_run_id)),
                &plan,
            )?;
            let receipt = apply_verified_flow(&repository, &snapshot, &resolution.verified, &key)
                .await
                .with_context(|| format!("apply flow {}", flow.flow_run_id))?;
            let persisted = private_files::write_json(
                &options
                    .output
                    .join(format!("{}.receipt.json", flow.flow_run_id)),
                &receipt,
            );
            // Keep every committed receipt available for rollback even when
            // persisting its private journal fails.
            receipts.push(receipt);
            persisted?;
        }
        for flow in &flows {
            let snapshot = repository
                .load_responses_history_flow(flow, &key)
                .await
                .with_context(|| format!("postscan load flow {}", flow.flow_run_id))?;
            ensure!(
                inspect_flow(&snapshot)
                    .with_context(|| format!("postscan inspect flow {}", flow.flow_run_id))?
                    .proofs
                    .iter()
                    .all(|p| p["version"] == 3),
                "postscan verify flow {}: legacy effective recovery proof remains",
                flow.flow_run_id
            );
        }
        private_files::write_json(
            &options.output.join("complete.json"),
            &json!({"mode":"apply", "migration_flows":receipts.len(), "excluded_flows":options.excluded_flows}),
        )?;
        Ok(())
    }
    .await;
    if let Err(error) = applied {
        let mut rollback_failed = false;
        for receipt in receipts.iter().rev() {
            if repository
                .rollback_responses_history_migration(receipt, &key)
                .await
                .is_err()
            {
                rollback_failed = true;
            }
        }
        let _ = private_files::write_json(
            &options.output.join("failed.json"),
            &json!({
                "rollback_failed":rollback_failed,
                "archive_recovery_required_if_commit_outcome_uncertain":true,
            }),
        );
        return Err(error.context("migration failed; keep writers stopped and inspect private receipts/archive before recovery"));
    }
    pool.close().await;
    Ok(())
}

fn empty(plan: &ResponsesHistoryMigrationPlan) -> bool {
    plan.documents.is_empty()
        && plan.contexts.is_empty()
        && plan.checkpoints.is_empty()
        && plan.continuations.is_empty()
}

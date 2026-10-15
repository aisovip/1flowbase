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
    let mut proofs = BTreeMap::<(uuid::Uuid, u64, u64, String), ScopedHistoryTarget>::new();
    let mut versions = BTreeMap::<u64, usize>::new();
    for flow in &flows {
        let snapshot = repository.load_responses_history_flow(flow, &key).await?;
        for proof in inspect_flow(&snapshot)?.proofs {
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
            "flows": flows.len(), "versions": versions, "distinct_proofs": proofs.len(),
        }),
    )?;
    let mut graph = HistorySourceGraph::new(&proofs.into_values().collect::<Vec<_>>())?;
    for flow in flows
        .iter()
        .filter(|_| versions.keys().any(|version| *version != 3))
    {
        let snapshot = repository.load_responses_history_flow(flow, &key).await?;
        let inspection = inspect_flow(&snapshot)?;
        drop(snapshot);
        sources::feed(&repository, &mut graph, flow, inspection, options.page_size).await?;
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
        let snapshot = repository.load_responses_history_flow(flow, &key).await?;
        let plan = build_plan(&snapshot, &resolution.verified)?;
        if empty(&plan) {
            continue;
        }
        repository
            .preview_responses_history_migration(&plan, &key)
            .await?;
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
            &json!({"mode":"dry-run", "migration_flows":planned.len()}),
        )?;
        return Ok(());
    }
    private_files::backup_database(&options.output, &url)?;
    let mut receipts: Vec<ResponsesHistoryMigrationReceipt> = Vec::new();
    let applied: Result<()> = async {
        for flow in &planned {
            let snapshot = repository.load_responses_history_flow(flow, &key).await?;
            let plan = build_plan(&snapshot, &resolution.verified)?;
            // fsync before commit. A process crash in the receipt window is explicit
            // in the journal and recoverable from before.dump with writers stopped.
            private_files::write_json(
                &options
                    .output
                    .join(format!("{}.intent.json", flow.flow_run_id)),
                &plan,
            )?;
            let receipt =
                apply_verified_flow(&repository, &snapshot, &resolution.verified, &key).await?;
            receipts.push(receipt);
            private_files::write_json(
                &options
                    .output
                    .join(format!("{}.receipt.json", flow.flow_run_id)),
                receipts.last().unwrap(),
            )?;
        }
        for flow in &flows {
            let snapshot = repository.load_responses_history_flow(flow, &key).await?;
            ensure!(
                inspect_flow(&snapshot)?
                    .proofs
                    .iter()
                    .all(|p| p["version"] == 3),
                "legacy effective recovery proof remains"
            );
        }
        private_files::write_json(
            &options.output.join("complete.json"),
            &json!({"mode":"apply", "migration_flows":receipts.len()}),
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

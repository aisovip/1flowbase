//! Offline maintenance adapter participants. The transaction coordinator is kept
//! separate so every participant can share one flow lock and one SQL transaction.
use super::*;
use control_plane_contracts::ports::responses_history_migration::*;

mod documents;
mod sources;

/// Must be the transaction entrance, before locking documents or canonical rows.
/// Offline operators must additionally stop old writers before maintenance.
#[allow(dead_code)] // Connected by the complete maintenance coordinator in WP002b.
pub(super) async fn lock_flow(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    plan: &ResponsesHistoryMigrationPlan,
) -> Result<()> {
    lock_application_run_native_projection(tx, plan.flow.flow_run_id).await?;
    let status = sqlx::query_scalar::<_, String>(
        "select status from flow_runs where id=$1 and scope_id=$2 and application_id=$3 for update",
    )
    .bind(plan.flow.flow_run_id)
    .bind(plan.flow.scope_id)
    .bind(plan.flow.application_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| anyhow!("responses_history_flow_owner_mismatch"))?;
    anyhow::ensure!(
        status == plan.expected_status.as_str() && status != "running",
        "responses_history_flow_status_conflict"
    );
    let live_claim = sqlx::query_scalar::<_, bool>(
        "select exists(select 1 from flow_run_resume_claims where flow_run_id=$1 and status='processing' and lease_expires_at>now())",
    )
    .bind(plan.flow.flow_run_id)
    .fetch_one(&mut **tx)
    .await?;
    anyhow::ensure!(!live_claim, "responses_history_live_resume_claim");
    let latest = sqlx::query_scalar::<_, Uuid>(
        "select id from runtime_context_projections where flow_run_id=$1 and projection_kind='context_version' order by context_sequence desc nulls last,created_at desc,id desc limit 1",
    )
    .bind(plan.flow.flow_run_id)
    .fetch_optional(&mut **tx)
    .await?;
    anyhow::ensure!(
        latest == plan.expected_latest_context_version_id,
        "responses_history_parent_context_conflict"
    );
    Ok(())
}

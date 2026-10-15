//! Offline maintenance adapter. One flow transaction owns every mutable change;
//! protocol proof verification remains in the control plane.
use super::*;
use control_plane_contracts::ports::responses_history_migration::*;

mod capsules;
mod checkpoints;
mod documents;
mod sources;

async fn latest_context(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    flow: &ResponsesHistoryFlowKey,
) -> Result<Option<Uuid>> {
    Ok(sqlx::query_scalar::<_,Uuid>("select id from runtime_context_projections where flow_run_id=$1 and scope_id=$2 and application_id=$3 and projection_kind='context_version' order by context_sequence desc nulls last,created_at desc,id desc limit 1")
        .bind(flow.flow_run_id).bind(flow.scope_id).bind(flow.application_id).fetch_optional(&mut **tx).await?)
}

/// Transaction entrance, before any source/queue/projection row. Offline callers
/// must additionally stop old writers; this port never changes writer admission.
async fn lock_flow(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    flow: &ResponsesHistoryFlowKey,
    expected_status: domain::FlowRunStatus,
) -> Result<()> {
    lock_application_run_native_projection(tx, flow.flow_run_id).await?;
    let status = sqlx::query_scalar::<_, String>(
        "select status from flow_runs where id=$1 and scope_id=$2 and application_id=$3 for update",
    )
    .bind(flow.flow_run_id)
    .bind(flow.scope_id)
    .bind(flow.application_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| anyhow!("responses_history_flow_owner_mismatch"))?;
    anyhow::ensure!(
        status == expected_status.as_str() && status != "running",
        "responses_history_flow_status_conflict"
    );
    let live=sqlx::query_scalar::<_,bool>("select exists(select 1 from flow_run_resume_claims where flow_run_id=$1 and status='processing' and lease_expires_at>now())")
        .bind(flow.flow_run_id).fetch_one(&mut **tx).await?;
    anyhow::ensure!(!live, "responses_history_live_resume_claim");
    Ok(())
}

async fn snapshot(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    flow: &ResponsesHistoryFlowKey,
    master_key: &str,
    lock: bool,
) -> Result<ResponsesHistoryFlowSnapshot> {
    let status = sqlx::query_scalar::<_, String>(
        "select status from flow_runs where id=$1 and scope_id=$2 and application_id=$3",
    )
    .bind(flow.flow_run_id)
    .bind(flow.scope_id)
    .bind(flow.application_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| anyhow!("responses_history_flow_owner_mismatch"))?;
    Ok(ResponsesHistoryFlowSnapshot {
        flow: *flow,
        status: serde_json::from_value(json!(status))?,
        latest_context_version_id: latest_context(tx, flow).await?,
        documents: documents::load_all(tx, flow).await?,
        checkpoints: checkpoints::load_all(tx, flow, lock).await?,
        continuations: capsules::load_all(tx, flow, master_key, lock).await?,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Before,
    After,
    Neutral,
}

fn merge_state(combined: &mut State, next: State) -> Result<()> {
    if next == State::Neutral {
        return Ok(());
    }
    anyhow::ensure!(
        *combined == State::Neutral || *combined == next,
        "responses_history_mixed_preimages"
    );
    *combined = next;
    Ok(())
}

fn exact_state(current: &Value, before: &Value, after: &Value) -> Result<State> {
    if current == before {
        Ok(if before == after {
            State::Neutral
        } else {
            State::Before
        })
    } else if current == after {
        Ok(State::After)
    } else {
        Err(anyhow!("responses_history_document_cas_conflict"))
    }
}

/// Locks and verifies all planned participants before any durable write.
async fn validate(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    plan: &ResponsesHistoryMigrationPlan,
    current: &ResponsesHistoryFlowSnapshot,
    master_key: &str,
) -> Result<State> {
    let mut state = State::Neutral;
    let mut owners = std::collections::HashSet::new();
    for change in &plan.documents {
        anyhow::ensure!(
            owners.insert(change.owner),
            "responses_history_duplicate_document"
        );
        let value = documents::read_current(tx, &plan.flow, change.owner, true).await?;
        merge_state(
            &mut state,
            exact_state(&value, &change.before, &change.after)?,
        )?;
        if let Some(shadow) = documents::read_shadow(tx, &plan.flow, change.owner).await? {
            documents::verify_shadow_content(tx, &plan.flow, &shadow, &value).await?;
        }
    }
    let mut checkpoint_ids = std::collections::HashSet::new();
    let mut after_heads = Vec::new();
    for change in &plan.checkpoints {
        anyhow::ensure!(
            checkpoint_ids.insert(change.checkpoint_id),
            "responses_history_duplicate_checkpoint"
        );
        checkpoints::verify_before(tx, &plan.flow, change).await?;
        let actual = current
            .checkpoints
            .iter()
            .find(|c| c.checkpoint_id == change.checkpoint_id)
            .ok_or_else(|| anyhow!("responses_history_checkpoint_owner_mismatch"))?;
        let before = actual.locator_payload == change.before_locator_payload
            && actual.variable_snapshot == change.before_variable_snapshot
            && actual.lineage.last().map(|e| &e.identity) == change.before_context.as_ref();
        if before {
            merge_state(&mut state, State::Before)?;
        } else {
            anyhow::ensure!(
                checkpoints::is_after(tx, &plan.flow, actual, change).await?,
                "responses_history_checkpoint_cas_conflict"
            );
            let entry = actual
                .lineage
                .last()
                .ok_or_else(|| anyhow!("responses_history_checkpoint_context_missing"))?;
            after_heads.push((entry.sequence, entry.identity.context_version_id));
            merge_state(&mut state, State::After)?;
        }
        let shadow = checkpoints::verify_current_shadow(tx, &plan.flow, actual).await?;
        if !before {
            anyhow::ensure!(
                shadow
                    .as_ref()
                    .is_none_or(|binding| binding.context_version_id.is_none()),
                "responses_history_checkpoint_shadow_context_conflict"
            );
        }
    }
    let mut slot_keys = std::collections::HashSet::new();
    for change in &plan.continuations {
        anyhow::ensure!(
            slot_keys.insert(&change.before.slot_key),
            "responses_history_duplicate_capsule"
        );
        capsules::authenticate(&plan.flow, &change.before, master_key)?;
        let actual = current
            .continuations
            .iter()
            .find(|c| c.slot_key == change.before.slot_key)
            .ok_or_else(|| anyhow!("responses_history_capsule_missing"))?;
        let next = if actual == &change.before {
            if change.before.payload == change.after_payload {
                State::Neutral
            } else {
                State::Before
            }
        } else {
            anyhow::ensure!(
                actual.slot_key == change.before.slot_key
                    && actual.key_version == change.before.key_version
                    && actual.hard_expires_at == change.before.hard_expires_at
                    && actual.payload == change.after_payload
                    && change.before.payload != change.after_payload,
                "responses_history_capsule_cas_conflict"
            );
            State::After
        };
        merge_state(&mut state, next)?;
    }
    if state == State::After && !after_heads.is_empty() {
        after_heads.sort_unstable();
        anyhow::ensure!(
            current.latest_context_version_id == after_heads.last().map(|(_, id)| *id),
            "responses_history_parent_context_conflict"
        );
        if let Some(old) = plan.expected_latest_context_version_id {
            let old = checkpoints::context(tx, &plan.flow, old).await?;
            anyhow::ensure!(
                after_heads
                    .iter()
                    .all(|(sequence, _)| *sequence > old.entry.sequence),
                "responses_history_checkpoint_sequence_conflict"
            );
        }
    } else {
        anyhow::ensure!(
            current.latest_context_version_id == plan.expected_latest_context_version_id,
            "responses_history_parent_context_conflict"
        );
    }
    Ok(state)
}

fn empty_receipt(
    plan: &ResponsesHistoryMigrationPlan,
    current: Option<Uuid>,
) -> ResponsesHistoryMigrationReceipt {
    ResponsesHistoryMigrationReceipt {
        format_version: 1,
        flow: plan.flow,
        expected_status: plan.expected_status,
        before_latest_context_version_id: current,
        after_latest_context_version_id: current,
        documents: Vec::new(),
        checkpoints: Vec::new(),
        continuations: Vec::new(),
    }
}

#[async_trait]
impl ResponsesHistoryMigrationRepository for PgControlPlaneStore {
    async fn load_responses_history_flow(
        &self,
        flow: &ResponsesHistoryFlowKey,
        master_key: &str,
    ) -> Result<ResponsesHistoryFlowSnapshot> {
        let mut tx = self.pool().begin().await?;
        sqlx::query("set transaction isolation level repeatable read, read only")
            .execute(&mut *tx)
            .await?;
        let current = snapshot(&mut tx, flow, master_key, false).await?;
        tx.rollback().await?;
        Ok(current)
    }

    async fn preview_responses_history_migration(
        &self,
        plan: &ResponsesHistoryMigrationPlan,
        master_key: &str,
    ) -> Result<()> {
        let mut tx = self.pool().begin().await?;
        lock_flow(&mut tx, &plan.flow, plan.expected_status).await?;
        let current = snapshot(&mut tx, &plan.flow, master_key, true).await?;
        validate(&mut tx, plan, &current, master_key).await?;
        tx.rollback().await?;
        Ok(())
    }

    async fn apply_responses_history_migration(
        &self,
        plan: &ResponsesHistoryMigrationPlan,
        master_key: &str,
    ) -> Result<ResponsesHistoryMigrationReceipt> {
        let mut tx = self.pool().begin().await?;
        lock_flow(&mut tx, &plan.flow, plan.expected_status).await?;
        let current = snapshot(&mut tx, &plan.flow, master_key, true).await?;
        let state = validate(&mut tx, plan, &current, master_key).await?;
        if state == State::After {
            tx.rollback().await?;
            return Ok(empty_receipt(plan, current.latest_context_version_id));
        }
        let documents = documents::apply(&mut tx, &plan.flow, &plan.documents).await?;
        let checkpoints = checkpoints::apply(&mut tx, &plan.flow, &plan.checkpoints).await?;
        let continuations =
            capsules::apply(&mut tx, &plan.flow, &plan.continuations, master_key).await?;
        let after_latest_context_version_id = latest_context(&mut tx, &plan.flow).await?;
        let receipt = ResponsesHistoryMigrationReceipt {
            format_version: 1,
            flow: plan.flow,
            expected_status: plan.expected_status,
            before_latest_context_version_id: current.latest_context_version_id,
            after_latest_context_version_id,
            documents,
            checkpoints,
            continuations,
        };
        tx.commit().await?;
        Ok(receipt)
    }

    async fn rollback_responses_history_migration(
        &self,
        receipt: &ResponsesHistoryMigrationReceipt,
        master_key: &str,
    ) -> Result<()> {
        anyhow::ensure!(
            receipt.format_version == 1,
            "responses_history_receipt_version_invalid"
        );
        let mut tx = self.pool().begin().await?;
        lock_flow(&mut tx, &receipt.flow, receipt.expected_status).await?;
        let current = snapshot(&mut tx, &receipt.flow, master_key, true).await?;
        anyhow::ensure!(
            current.latest_context_version_id == receipt.after_latest_context_version_id,
            "responses_history_parent_context_conflict"
        );
        let mut checkpoint_ids = std::collections::HashSet::new();
        for item in &receipt.checkpoints {
            anyhow::ensure!(
                checkpoint_ids.insert(item.checkpoint_id),
                "responses_history_duplicate_checkpoint"
            );
        }
        let mut slots = std::collections::HashSet::new();
        for item in &receipt.continuations {
            anyhow::ensure!(
                slots.insert(&item.after.slot_key),
                "responses_history_duplicate_capsule"
            );
            anyhow::ensure!(
                current.continuations.iter().any(|slot| slot == &item.after),
                "responses_history_capsule_rollback_conflict"
            );
        }
        documents::rollback(&mut tx, &receipt.flow, &receipt.documents).await?;
        checkpoints::rollback(
            &mut tx,
            &receipt.flow,
            &current.checkpoints,
            &receipt.checkpoints,
        )
        .await?;
        capsules::rollback(&mut tx, &receipt.flow, &receipt.continuations, master_key).await?;
        // Appended immutable contexts deliberately remain the sequence head. Only
        // mutable checkpoint references move back; later plans use the new max.
        tx.commit().await?;
        Ok(())
    }
}

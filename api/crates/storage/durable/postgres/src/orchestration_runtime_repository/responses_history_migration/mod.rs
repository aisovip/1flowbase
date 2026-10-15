//! Offline maintenance adapter. One flow transaction owns every mutable change;
//! protocol proof verification remains in the control plane.
use super::*;
use control_plane_contracts::ports::responses_history_migration::*;

mod capsules;
mod contexts;
use std::collections::{HashMap, HashSet};
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
    let latest_context_version_id = latest_context(tx, flow).await?;
    let mut checkpoints = checkpoints::load_all(tx, flow, lock).await?;
    let mut roots = Vec::new();
    if let Some(id) = latest_context_version_id {
        roots.push(id);
    }
    for checkpoint in &checkpoints {
        if let Some(id) = checkpoints::locator_context(&checkpoint.locator_payload)? {
            roots.push(id);
        }
    }
    let contexts = contexts::load_dag(tx, flow, roots).await?;
    let index: HashMap<_, _> = contexts
        .iter()
        .map(|entry| (entry.identity.context_version_id, &entry.identity))
        .collect();
    for checkpoint in &mut checkpoints {
        checkpoint.context = checkpoints::locator_context(&checkpoint.locator_payload)?
            .map(|id| {
                index
                    .get(&id)
                    .cloned()
                    .cloned()
                    .ok_or_else(|| anyhow!("responses_history_checkpoint_context_missing"))
            })
            .transpose()?;
    }
    Ok(ResponsesHistoryFlowSnapshot {
        flow: *flow,
        status: serde_json::from_value(json!(status))?,
        latest_context_version_id,
        documents: documents::load_all(tx, flow).await?,
        contexts,
        checkpoints,
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

/// Validate the source DAG once, retaining sharing. Old immutable nodes remain
/// readable after apply and provide exact CAS preimages for retry recognition.
async fn validate_context_plan(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    plan: &ResponsesHistoryMigrationPlan,
    current: &ResponsesHistoryFlowSnapshot,
) -> Result<Option<i64>> {
    let checkpoint_positions: HashMap<_, _> = current
        .checkpoints
        .iter()
        .enumerate()
        .map(|(index, c)| (c.checkpoint_id, index))
        .collect();
    let checkpoint_changes: HashMap<_, _> = plan
        .checkpoints
        .iter()
        .map(|change| (change.checkpoint_id, change))
        .collect();
    anyhow::ensure!(
        checkpoint_changes.len() == plan.checkpoints.len(),
        "responses_history_duplicate_checkpoint"
    );
    let mut roots = Vec::new();
    if let Some(id) = plan.expected_latest_context_version_id {
        roots.push(id);
    }
    for checkpoint in &current.checkpoints {
        let before = checkpoint_changes
            .get(&checkpoint.checkpoint_id)
            .map_or(checkpoint.context.as_ref(), |change| {
                change.before_context.as_ref()
            });
        if let Some(identity) = before {
            roots.push(identity.context_version_id);
        }
    }
    let old = contexts::load_dag(tx, &plan.flow, roots).await?;
    let old_index: HashMap<_, _> = old
        .iter()
        .map(|entry| (entry.identity.context_version_id, entry))
        .collect();
    let planned_ids: HashSet<_> = plan
        .contexts
        .iter()
        .map(|change| change.before.identity.context_version_id)
        .collect();
    anyhow::ensure!(
        planned_ids.len() == plan.contexts.len(),
        "responses_history_duplicate_context"
    );
    let mut earlier = HashSet::new();
    for change in &plan.contexts {
        let id = change.before.identity.context_version_id;
        anyhow::ensure!(
            old_index.get(&id).is_some_and(|old| **old == change.before),
            "responses_history_context_preimage_conflict"
        );
        anyhow::ensure!(
            change.after_content.is_object()
                && change.after_content.get("format") == change.before.content.get("format"),
            "responses_history_context_encoding_changed"
        );
        if let Some(parent) = change.before.previous_projection_id {
            anyhow::ensure!(
                !planned_ids.contains(&parent) || earlier.contains(&parent),
                "responses_history_context_order_invalid"
            );
        }
        earlier.insert(id);
    }
    for checkpoint in &current.checkpoints {
        if !checkpoint_changes.contains_key(&checkpoint.checkpoint_id) {
            anyhow::ensure!(
                checkpoint
                    .context
                    .as_ref()
                    .is_none_or(|identity| !planned_ids.contains(&identity.context_version_id)),
                "responses_history_checkpoint_remap_missing"
            );
        }
    }
    for entry in &old {
        if entry
            .previous_projection_id
            .is_some_and(|id| planned_ids.contains(&id))
        {
            anyhow::ensure!(
                planned_ids.contains(&entry.identity.context_version_id),
                "responses_history_context_descendant_missing"
            );
        }
    }
    let mut previous_index = None;
    let mut has_inline = false;
    for change in &plan.checkpoints {
        let index = *checkpoint_positions
            .get(&change.checkpoint_id)
            .ok_or_else(|| anyhow!("responses_history_checkpoint_owner_mismatch"))?;
        anyhow::ensure!(
            previous_index.is_none_or(|previous| index > previous),
            "responses_history_checkpoint_order_invalid"
        );
        previous_index = Some(index);
        anyhow::ensure!(
            checkpoints::locator_context(&change.before_locator_payload)?
                == change.before_context.as_ref().map(|c| c.context_version_id),
            "responses_history_checkpoint_preimage_context_invalid"
        );
        if let Some(identity) = &change.before_context {
            anyhow::ensure!(
                old_index
                    .get(&identity.context_version_id)
                    .is_some_and(|entry| entry.identity == *identity)
                    && planned_ids.contains(&identity.context_version_id),
                "responses_history_checkpoint_context_mapping_missing"
            );
        }
        has_inline |= checkpoints::inline_content(change)?.is_some();
    }
    let inserts = !plan.contexts.is_empty() || has_inline;
    if inserts {
        if let Some(latest) = plan.expected_latest_context_version_id {
            anyhow::ensure!(
                plan.contexts.last().is_some_and(|change| change
                    .before
                    .identity
                    .context_version_id
                    == latest),
                "responses_history_latest_context_anchor_missing"
            );
        } else {
            anyhow::ensure!(
                has_inline
                    && plan.checkpoints.last().map(|c| c.checkpoint_id)
                        == current.checkpoints.last().map(|c| c.checkpoint_id),
                "responses_history_latest_checkpoint_anchor_missing"
            );
        }
    }
    Ok(plan
        .expected_latest_context_version_id
        .and_then(|id| old_index.get(&id).map(|entry| entry.sequence)))
}

/// Locks and verifies every mutable participant before the first durable write.
async fn validate(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    plan: &ResponsesHistoryMigrationPlan,
    current: &ResponsesHistoryFlowSnapshot,
    master_key: &str,
) -> Result<State> {
    let minimum_sequence = validate_context_plan(tx, plan, current).await?;
    let mut state = State::Neutral;
    let mut owners = HashSet::new();
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
    let current_index: HashMap<_, _> = current
        .contexts
        .iter()
        .map(|entry| (entry.identity.context_version_id, entry))
        .collect();
    let checkpoint_index: HashMap<_, _> = current
        .checkpoints
        .iter()
        .map(|c| (c.checkpoint_id, c))
        .collect();
    let mut seeds = Vec::new();
    let mut last_inline_after = None;
    for change in &plan.checkpoints {
        let actual = checkpoint_index
            .get(&change.checkpoint_id)
            .ok_or_else(|| anyhow!("responses_history_checkpoint_owner_mismatch"))?;
        let before = checkpoints::is_before(actual, change);
        if before {
            merge_state(&mut state, State::Before)?;
        } else {
            let identity = actual
                .context
                .as_ref()
                .ok_or_else(|| anyhow!("responses_history_checkpoint_context_missing"))?;
            let entry = current_index
                .get(&identity.context_version_id)
                .ok_or_else(|| anyhow!("responses_history_checkpoint_context_missing"))?;
            anyhow::ensure!(
                checkpoints::has_after_projection(actual, change, entry)?,
                "responses_history_checkpoint_cas_conflict"
            );
            if let Some(old) = &change.before_context {
                seeds.push((old.context_version_id, identity.context_version_id));
            } else {
                anyhow::ensure!(
                    entry.previous_projection_id.is_none()
                        && Some(&entry.content) == checkpoints::inline_content(change)?.as_ref(),
                    "responses_history_inline_context_conflict"
                );
                let metadata=sqlx::query_as::<_,(Option<String>,Option<String>)>("select transition_kind,transition_actor from runtime_context_projections where id=$1")
                    .bind(identity.context_version_id).fetch_one(&mut **tx).await?;
                anyhow::ensure!(
                    metadata.0.as_deref() == Some("append")
                        && metadata.1.as_deref() == Some("host"),
                    "responses_history_inline_context_transition_conflict"
                );
                last_inline_after = Some(identity.context_version_id);
            }
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
    let mut slot_keys = HashSet::new();
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
    let inserts = !plan.contexts.is_empty()
        || plan
            .checkpoints
            .iter()
            .any(|c| c.legacy_inline_variable_pool.is_some());
    if inserts {
        let context_state =
            if current.latest_context_version_id == plan.expected_latest_context_version_id {
                State::Before
            } else {
                if let Some(old) = plan.expected_latest_context_version_id {
                    let new = current
                        .latest_context_version_id
                        .ok_or_else(|| anyhow!("responses_history_latest_context_missing"))?;
                    seeds.push((old, new));
                } else {
                    anyhow::ensure!(
                        current.latest_context_version_id == last_inline_after,
                        "responses_history_parent_context_conflict"
                    );
                }
                anyhow::ensure!(
                    contexts::match_after(tx, &plan.flow, &plan.contexts, seeds, minimum_sequence)
                        .await?,
                    "responses_history_context_cas_conflict"
                );
                State::After
            };
        merge_state(&mut state, context_state)?;
    } else {
        anyhow::ensure!(
            current.latest_context_version_id == plan.expected_latest_context_version_id,
            "responses_history_parent_context_conflict"
        );
    }
    Ok(state)
}

fn snapshot_latest(
    current: &ResponsesHistoryFlowSnapshot,
) -> Option<ResponsesHistoryContextIdentity> {
    current.latest_context_version_id.and_then(|id| {
        current
            .contexts
            .iter()
            .find(|entry| entry.identity.context_version_id == id)
            .map(|entry| entry.identity.clone())
    })
}

fn empty_receipt(
    plan: &ResponsesHistoryMigrationPlan,
    current: Option<ResponsesHistoryContextIdentity>,
) -> ResponsesHistoryMigrationReceipt {
    ResponsesHistoryMigrationReceipt {
        format_version: 2,
        flow: plan.flow,
        expected_status: plan.expected_status,
        before_latest_context: current.clone(),
        after_latest_context: current,
        documents: Vec::new(),
        contexts: Vec::new(),
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
            return Ok(empty_receipt(plan, snapshot_latest(&current)));
        }
        let documents = documents::apply(&mut tx, &plan.flow, &plan.documents).await?;
        let mut mapping = HashMap::new();
        let mut context_receipts = Vec::with_capacity(plan.contexts.len());
        let preserve_latest =
            plan.expected_latest_context_version_id.is_some() && !plan.contexts.is_empty();
        let split = plan.contexts.len() - usize::from(preserve_latest);
        for change in &plan.contexts[..split] {
            context_receipts
                .push(contexts::copy_one(&mut tx, &plan.flow, change, &mut mapping).await?);
        }
        let inline = checkpoints::append_inline(&mut tx, &plan.flow, &plan.checkpoints).await?;
        for change in &plan.contexts[split..] {
            context_receipts
                .push(contexts::copy_one(&mut tx, &plan.flow, change, &mut mapping).await?);
        }
        let checkpoints =
            checkpoints::apply(&mut tx, &plan.flow, &plan.checkpoints, &mapping, &inline).await?;
        let continuations =
            capsules::apply(&mut tx, &plan.flow, &plan.continuations, master_key).await?;
        let after_latest = latest_context(&mut tx, &plan.flow).await?;
        let after_latest_context = match after_latest {
            Some(id) => Some(
                contexts::context(&mut tx, &plan.flow, id)
                    .await?
                    .entry
                    .identity,
            ),
            None => None,
        };
        let receipt = ResponsesHistoryMigrationReceipt {
            format_version: 2,
            flow: plan.flow,
            expected_status: plan.expected_status,
            before_latest_context: snapshot_latest(&current),
            after_latest_context,
            documents,
            contexts: context_receipts,
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
            receipt.format_version == 2,
            "responses_history_receipt_version_invalid"
        );
        let mut tx = self.pool().begin().await?;
        lock_flow(&mut tx, &receipt.flow, receipt.expected_status).await?;
        let current = snapshot(&mut tx, &receipt.flow, master_key, true).await?;
        anyhow::ensure!(
            snapshot_latest(&current) == receipt.after_latest_context,
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
        contexts::verify_receipts(&mut tx, &receipt.flow, &receipt.contexts).await?;
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

#[cfg(test)]
#[path = "../../_tests/responses_history_migration/mod.rs"]
mod tests;

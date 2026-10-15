//! Explicit-location planner for the offline migration. User history, opaque
//! payloads and arbitrary proof-looking objects are never recursively rewritten.
use std::collections::{BTreeMap, BTreeSet};

use anyhow::{ensure, Context, Result};
use control_plane_contracts::ports::responses_history_migration::*;
use domain::orchestration::ResponsesRoundEvidence;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use super::{History, SourceCoverage, SourcedReplacement};

const REQUEST_INPUT_PROOF: &str = "/sys/responses_input_history";
const SYS_PROOFS: &[&str] = &[
    REQUEST_INPUT_PROOF,
    "/sys/native_inference_recovery/history",
];
const CALLBACK_PROOFS: &[&str] = &[
    "/provider_metadata/native_response/history",
    "/responses_round/history",
];
const ROUND_PROOF: &[&str] = &["/responses_round/history"];
const SPAN_PROOF: &[&str] = &["/native_response/history"];
const CONTINUATION_PROOF: &[&str] = &["/native_history"];
const MARKER: &str = "__runtime_recovery_context";
type ProofKey = (u32, usize, String);

#[derive(Debug, Clone, Serialize)]
pub struct FlowInspection {
    /// All deduplicated valid owned proofs, including current format, for inventory.
    /// Null/absent leaves are not proofs and require no conversion.
    pub proofs: Vec<Value>,
    /// Exact host-owned request-input slots in FlowInput and checkpoint sys copies.
    pub request_input_histories: Vec<Value>,
    /// Every proof with at least one owned occurrence outside the input contract.
    /// A value may also occur in request_input_histories and still require full source.
    pub full_source_required_proofs: Vec<Value>,
    /// Actual persisted round output, in its original order, from owned locations.
    pub public_rounds: Vec<ResponsesRoundEvidence>,
}

fn parse_proof(value: &Value) -> Result<History> {
    let proof: History = serde_json::from_value(value.clone())
        .map_err(|_| anyhow::anyhow!("responses_history_owned_proof_invalid"))?;
    ensure!(
        proof.digest.len() == 64 && proof.digest.bytes().all(|c| c.is_ascii_hexdigit()),
        "responses_history_owned_proof_invalid"
    );
    ensure!(
        matches!(proof.version, 1 | 2 | 3),
        "native_history_version_unsupported"
    );
    Ok(proof)
}

fn proof_key(proof: &History) -> ProofKey {
    (proof.version, proof.item_count, proof.digest.clone())
}

fn document_paths(owner: ResponsesHistoryDocumentOwner) -> &'static [&'static str] {
    match owner {
        ResponsesHistoryDocumentOwner::FlowInput => SYS_PROOFS,
        ResponsesHistoryDocumentOwner::FlowOutput => ROUND_PROOF,
        ResponsesHistoryDocumentOwner::CallbackRequest { .. }
        | ResponsesHistoryDocumentOwner::NodeOutput { .. } => CALLBACK_PROOFS,
        ResponsesHistoryDocumentOwner::SpanMetadata { .. } => SPAN_PROOF,
    }
}

fn visit_paths(
    value: &Value,
    paths: &[&str],
    visitor: &mut impl FnMut(&Value, SourceCoverage) -> Result<()>,
) -> Result<()> {
    for path in paths {
        if let Some(proof) = value.pointer(path).filter(|proof| !proof.is_null()) {
            parse_proof(proof).with_context(|| format!("owned proof location {path}"))?;
            visitor(
                proof,
                if *path == REQUEST_INPUT_PROOF {
                    SourceCoverage::RequestInput
                } else {
                    SourceCoverage::FullHistory
                },
            )?;
        }
    }
    Ok(())
}

fn visit_pool(
    pool: &Value,
    visitor: &mut impl FnMut(&Value, SourceCoverage) -> Result<()>,
) -> Result<()> {
    let object = pool
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("responses_history_checkpoint_pool_invalid"))?;
    visit_paths(pool, SYS_PROOFS, visitor)?;
    for (name, node) in object {
        if name == "sys" || name == MARKER {
            continue;
        }
        if let Some(callback) = node.get("__llm_tool_callback") {
            visit_paths(callback, CALLBACK_PROOFS, visitor)?;
        }
    }
    Ok(())
}

fn collect_round(value: &Value, rounds: &mut Vec<ResponsesRoundEvidence>) -> Result<()> {
    if let Some(round) = value
        .get("responses_round")
        .filter(|round| !round.is_null())
    {
        // Null proof is a legitimate record with no complete attestation. It is
        // preserved by the planner and is not a source for a verified public chain.
        if round.get("history").is_none_or(Value::is_null) {
            return Ok(());
        }
        let round: ResponsesRoundEvidence = serde_json::from_value(round.clone())
            .map_err(|_| anyhow::anyhow!("responses_history_public_round_invalid"))?;
        parse_proof(&round.history)?;
        ensure!(
            !round.response_id.is_empty(),
            "responses_history_public_round_invalid"
        );
        rounds.push(round);
    }
    Ok(())
}

fn inline_pool(checkpoint: &ResponsesHistoryCheckpoint) -> Result<&Value> {
    ensure!(
        checkpoint.context.is_none(),
        "responses_history_checkpoint_not_inline"
    );
    let pool = checkpoint
        .variable_snapshot
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("responses_history_checkpoint_pool_invalid"))?;
    ensure!(
        !(pool.len() == 1 && pool.contains_key(MARKER)),
        "responses_history_checkpoint_context_missing"
    );
    Ok(&checkpoint.variable_snapshot)
}

/// Validate all loaded topology/identities, then identify recovery-owned contexts
/// from checkpoint references. The implicit latest head may instead belong to a
/// provider observation lineage whose content must remain opaque to migration.
fn validate_graph(snapshot: &ResponsesHistoryFlowSnapshot) -> Result<BTreeSet<Uuid>> {
    let mut nodes = BTreeMap::new();
    for entry in &snapshot.contexts {
        let id = entry.identity.context_version_id;
        ensure!(
            !nodes.contains_key(&id),
            "responses_history_context_duplicate"
        );
        if let Some(parent) = entry.previous_projection_id {
            let parent: &&ResponsesHistoryContextLineageEntry =
                nodes.get(&parent).ok_or_else(|| {
                    anyhow::anyhow!("responses_history_context_parent_missing_or_not_topological")
                })?;
            ensure!(
                parent.sequence < entry.sequence,
                "responses_history_context_sequence_invalid"
            );
        }
        nodes.insert(id, entry);
    }
    match snapshot.latest_context_version_id {
        Some(id) => {
            let latest = nodes
                .get(&id)
                .ok_or_else(|| anyhow::anyhow!("responses_history_context_latest_missing"))?;
            ensure!(
                snapshot
                    .contexts
                    .iter()
                    .all(|entry| entry.sequence <= latest.sequence),
                "responses_history_context_latest_not_maximum"
            );
        }
        None => ensure!(nodes.is_empty(), "responses_history_context_latest_missing"),
    }
    let mut checkpoint_contexts = BTreeSet::new();
    for checkpoint in &snapshot.checkpoints {
        ensure!(
            checkpoint.variable_snapshot.is_object(),
            "responses_history_checkpoint_snapshot_invalid"
        );
        let locator = checkpoint
            .locator_payload
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("responses_history_checkpoint_locator_invalid"))?;
        let locator_id = locator
            .get("context_version_id")
            .map(|id| {
                let text = id.as_str().ok_or_else(|| {
                    anyhow::anyhow!("responses_history_checkpoint_context_invalid")
                })?;
                Ok::<_, anyhow::Error>(Uuid::parse_str(text)?)
            })
            .transpose()?;
        match &checkpoint.context {
            Some(identity) => {
                ensure!(
                    locator_id == Some(identity.context_version_id),
                    "responses_history_checkpoint_context_mismatch"
                );
                let node = nodes.get(&identity.context_version_id).ok_or_else(|| {
                    anyhow::anyhow!("responses_history_checkpoint_context_missing")
                })?;
                ensure!(
                    &node.identity == identity,
                    "responses_history_checkpoint_context_identity_mismatch"
                );
                // Shared ancestors are visited once. Reachability, never payload
                // shape, establishes which context content recovery actually reads.
                let mut next = Some(identity.context_version_id);
                while let Some(id) = next {
                    if !checkpoint_contexts.insert(id) {
                        break;
                    }
                    let entry = nodes.get(&id).ok_or_else(|| {
                        anyhow::anyhow!("responses_history_checkpoint_context_missing")
                    })?;
                    ensure!(
                        matches!(
                            entry.content.get("format").and_then(Value::as_str),
                            Some("runtime_snapshot_v1" | "runtime_delta_v1")
                        ),
                        "responses_history_context_format_unsupported"
                    );
                    if entry.previous_projection_id.is_none() {
                        ensure!(
                            entry.content.get("format").and_then(Value::as_str)
                                == Some("runtime_snapshot_v1"),
                            "responses_history_context_root_snapshot_missing"
                        );
                    }
                    next = entry.previous_projection_id;
                }
            }
            None => {
                ensure!(
                    locator_id.is_none(),
                    "responses_history_checkpoint_context_missing"
                );
                inline_pool(checkpoint)?;
            }
        }
    }
    Ok(checkpoint_contexts)
}

fn visit_node(
    node: &Value,
    visitor: &mut impl FnMut(&Value, SourceCoverage) -> Result<()>,
    rounds: &mut Vec<ResponsesRoundEvidence>,
) -> Result<()> {
    // Only reserved callback state is recovery-owned. Direct node output fields
    // remain user data even when they resemble native or public proof envelopes.
    if let Some(callback) = node.get("__llm_tool_callback") {
        visit_paths(callback, CALLBACK_PROOFS, visitor)?;
        collect_round(callback, rounds)?;
    }
    Ok(())
}

fn visit_context(
    content: &Value,
    visitor: &mut impl FnMut(&Value, SourceCoverage) -> Result<()>,
    rounds: &mut Vec<ResponsesRoundEvidence>,
) -> Result<()> {
    match content.get("format").and_then(Value::as_str) {
        Some("runtime_snapshot_v1") | Some("runtime_delta_v1") => {
            let field = if content["format"] == "runtime_snapshot_v1" {
                "variable_pool"
            } else {
                "set"
            };
            let pool = content
                .get(field)
                .filter(|value| value.is_object())
                .ok_or_else(|| anyhow::anyhow!("responses_history_context_pool_invalid"))?;
            visit_pool(pool, visitor)?;
            for (name, node) in pool
                .as_object()
                .ok_or_else(|| anyhow::anyhow!("responses_history_context_pool_invalid"))?
            {
                if name != "sys" && name != MARKER {
                    if let Some(callback) = node.get("__llm_tool_callback") {
                        collect_round(callback, rounds)?;
                    }
                }
            }
            if field == "set" {
                for append in context_appends(content)? {
                    for field in ["state_set", "state_overlay"] {
                        if let Some(state) = append.get(field) {
                            visit_paths(state, CALLBACK_PROOFS, visitor)?;
                            collect_round(state, rounds)?;
                        }
                    }
                    for field in ["node_set", "node_overlay"] {
                        if let Some(node) = append.get(field) {
                            visit_node(node, visitor, rounds)?;
                        }
                    }
                }
            }
        }
        _ => anyhow::bail!("responses_history_context_format_unsupported"),
    }
    Ok(())
}

fn context_appends(content: &Value) -> Result<&[Value]> {
    match content.get("llm_callback_appends") {
        None => Ok(&[]),
        Some(Value::Array(appends)) => Ok(appends),
        _ => anyhow::bail!("responses_history_context_appends_invalid"),
    }
}

pub fn inspect_flow(snapshot: &ResponsesHistoryFlowSnapshot) -> Result<FlowInspection> {
    let checkpoint_contexts = validate_graph(snapshot)?;
    let mut proofs = BTreeMap::new();
    let mut request_input_histories = BTreeMap::new();
    let mut full_source_required_proofs = BTreeMap::new();
    let mut rounds = Vec::new();
    let mut collect = |value: &Value, coverage: SourceCoverage| -> Result<()> {
        let proof = parse_proof(value)?;
        let key = proof_key(&proof);
        proofs.insert(key.clone(), value.clone());
        match coverage {
            SourceCoverage::RequestInput => {
                request_input_histories.insert(key, value.clone());
            }
            SourceCoverage::FullHistory => {
                full_source_required_proofs.insert(key, value.clone());
            }
        }
        Ok(())
    };
    for document in &snapshot.documents {
        visit_paths(
            &document.value,
            document_paths(document.owner),
            &mut collect,
        )?;
        if matches!(
            document.owner,
            ResponsesHistoryDocumentOwner::FlowOutput
                | ResponsesHistoryDocumentOwner::CallbackRequest { .. }
                | ResponsesHistoryDocumentOwner::NodeOutput { .. }
        ) {
            collect_round(&document.value, &mut rounds)?;
        }
    }
    for context in &snapshot.contexts {
        if checkpoint_contexts.contains(&context.identity.context_version_id) {
            visit_context(&context.content, &mut collect, &mut rounds)?;
        }
    }
    for checkpoint in &snapshot.checkpoints {
        if checkpoint.context.is_none() {
            let pool = inline_pool(checkpoint)?;
            visit_pool(pool, &mut collect)?;
            for (name, node) in pool
                .as_object()
                .ok_or_else(|| anyhow::anyhow!("responses_history_checkpoint_pool_invalid"))?
            {
                if name != "sys" && name != MARKER {
                    if let Some(callback) = node.get("__llm_tool_callback") {
                        collect_round(callback, &mut rounds)?;
                    }
                }
            }
        }
    }
    for continuation in &snapshot.continuations {
        visit_paths(&continuation.payload, CONTINUATION_PROOF, &mut collect)?;
    }
    Ok(FlowInspection {
        proofs: proofs.into_values().collect(),
        request_input_histories: request_input_histories.into_values().collect(),
        full_source_required_proofs: full_source_required_proofs.into_values().collect(),
        public_rounds: rounds,
    })
}

/// Collect all valid owned proof formats; callers can inventory current proofs
/// while the source graph ignores them and the planner leaves them unchanged.
pub fn collect_proofs(snapshot: &ResponsesHistoryFlowSnapshot) -> Result<Vec<Value>> {
    Ok(inspect_flow(snapshot)?.proofs)
}

pub fn public_rounds(
    snapshot: &ResponsesHistoryFlowSnapshot,
) -> Result<Vec<ResponsesRoundEvidence>> {
    Ok(inspect_flow(snapshot)?.public_rounds)
}

struct Replacements(BTreeMap<ProofKey, (Value, Value, SourceCoverage)>);

impl Replacements {
    fn new(flow_run_id: Uuid, verified: &[SourcedReplacement]) -> Result<Self> {
        let mut replacements = BTreeMap::<ProofKey, (Value, Value, SourceCoverage)>::new();
        for sourced in verified {
            if sourced.source.flow_run_id != flow_run_id {
                continue;
            }
            let old = sourced.replacement.old();
            let current = sourced.replacement.current();
            let parsed = parse_proof(&old)?;
            ensure!(
                matches!(parsed.version, 1 | 2),
                "responses_history_replacement_old_format_invalid"
            );
            let current_proof = parse_proof(&current)?;
            ensure!(
                current_proof.version == 3 && current_proof.item_count == parsed.item_count,
                "responses_history_replacement_current_invalid"
            );
            let key = proof_key(&parsed);
            if let Some((existing_old, existing_current, coverage)) = replacements.get_mut(&key) {
                ensure!(
                    &*existing_old == &old && &*existing_current == &current,
                    "responses_history_replacement_ambiguous"
                );
                if sourced.coverage() == SourceCoverage::FullHistory {
                    *coverage = SourceCoverage::FullHistory;
                }
            } else {
                replacements.insert(key, (old, current, sourced.coverage()));
            }
        }
        Ok(Self(replacements))
    }

    fn replace_paths(&self, value: &mut Value, paths: &[&str]) -> Result<()> {
        for path in paths {
            let Some(proof) = value.pointer_mut(path).filter(|proof| !proof.is_null()) else {
                continue;
            };
            let parsed =
                parse_proof(proof).with_context(|| format!("owned proof location {path}"))?;
            if parsed.version == 3 {
                continue;
            }
            let (old, current, coverage) = self.0.get(&proof_key(&parsed)).ok_or_else(|| {
                anyhow::anyhow!("responses_history_verified_replacement_missing:{path}")
            })?;
            ensure!(
                *coverage == SourceCoverage::FullHistory || *path == REQUEST_INPUT_PROOF,
                "responses_history_full_source_required:{path}"
            );
            ensure!(
                &*proof == old,
                "responses_history_verified_replacement_mismatch"
            );
            *proof = current.clone();
        }
        Ok(())
    }

    fn replace_pool(&self, pool: &mut Value) -> Result<()> {
        self.replace_paths(pool, SYS_PROOFS)?;
        let object = pool
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("responses_history_checkpoint_pool_invalid"))?;
        for (name, node) in object {
            if name == "sys" || name == MARKER {
                continue;
            }
            if let Some(callback) = node.get_mut("__llm_tool_callback") {
                self.replace_paths(callback, CALLBACK_PROOFS)?;
            }
        }
        Ok(())
    }
    fn replace_node(&self, node: &mut Value) -> Result<()> {
        if let Some(callback) = node.get_mut("__llm_tool_callback") {
            self.replace_paths(callback, CALLBACK_PROOFS)?;
        }
        Ok(())
    }

    fn replace_context(&self, content: &mut Value) -> Result<()> {
        let field = match content.get("format").and_then(Value::as_str) {
            Some("runtime_snapshot_v1") => "variable_pool",
            Some("runtime_delta_v1") => "set",
            _ => anyhow::bail!("responses_history_context_format_unsupported"),
        };
        let pool = content
            .get_mut(field)
            .ok_or_else(|| anyhow::anyhow!("responses_history_context_pool_invalid"))?;
        self.replace_pool(pool)?;
        if field == "set" {
            if let Some(appends) = content.get_mut("llm_callback_appends") {
                for append in appends
                    .as_array_mut()
                    .ok_or_else(|| anyhow::anyhow!("responses_history_context_appends_invalid"))?
                {
                    for field in ["state_set", "state_overlay"] {
                        if let Some(state) = append.get_mut(field) {
                            self.replace_paths(state, CALLBACK_PROOFS)?;
                        }
                    }
                    for field in ["node_set", "node_overlay"] {
                        if let Some(node) = append.get_mut(field) {
                            self.replace_node(node)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

/// Build only from typed verification receipts. Re-run after reloading the flow;
/// storage checks the exact before-values and immutable context identity under lock.
pub fn build_plan(
    snapshot: &ResponsesHistoryFlowSnapshot,
    verified: &[SourcedReplacement],
) -> Result<ResponsesHistoryMigrationPlan> {
    let checkpoint_contexts = validate_graph(snapshot)?;
    let replacements = Replacements::new(snapshot.flow.flow_run_id, verified)?;
    let mut plan = ResponsesHistoryMigrationPlan {
        flow: snapshot.flow,
        expected_status: snapshot.status,
        expected_latest_context_version_id: snapshot.latest_context_version_id,
        documents: Vec::new(),
        contexts: Vec::new(),
        checkpoints: Vec::new(),
        continuations: Vec::new(),
    };
    for document in &snapshot.documents {
        let mut after = document.value.clone();
        replacements.replace_paths(&mut after, document_paths(document.owner))?;
        if after != document.value {
            plan.documents.push(ResponsesHistoryDocumentChange {
                owner: document.owner,
                before: document.value.clone(),
                after,
            });
        }
    }
    let mut remapped = BTreeSet::new();
    for context in &snapshot.contexts {
        let mut after_content = context.content.clone();
        if checkpoint_contexts.contains(&context.identity.context_version_id) {
            replacements.replace_context(&mut after_content)?;
        }
        if after_content != context.content
            || context
                .previous_projection_id
                .is_some_and(|parent| remapped.contains(&parent))
        {
            remapped.insert(context.identity.context_version_id);
            plan.contexts.push(ResponsesHistoryContextChange {
                before: context.clone(),
                after_content,
            });
        }
    }
    let mut inline = BTreeMap::new();
    for checkpoint in &snapshot.checkpoints {
        if checkpoint.context.is_none() {
            let mut pool = inline_pool(checkpoint)?.clone();
            replacements.replace_pool(&mut pool)?;
            if &pool != inline_pool(checkpoint)? {
                inline.insert(checkpoint.checkpoint_id, pool);
            }
        }
    }
    if !remapped.is_empty() || !inline.is_empty() {
        if let Some(latest) = snapshot.latest_context_version_id {
            if let Some(position) = plan
                .contexts
                .iter()
                .position(|change| change.before.identity.context_version_id == latest)
            {
                let anchor = plan.contexts.remove(position);
                plan.contexts.push(anchor);
            } else {
                let context = snapshot
                    .contexts
                    .iter()
                    .find(|context| context.identity.context_version_id == latest)
                    .ok_or_else(|| anyhow::anyhow!("responses_history_context_latest_missing"))?;
                plan.contexts.push(ResponsesHistoryContextChange {
                    before: context.clone(),
                    after_content: context.content.clone(),
                });
                remapped.insert(latest);
            }
        } else if let Some(latest_checkpoint) = snapshot.checkpoints.last() {
            // Preserve repository latest selection when an earlier inline checkpoint
            // migrates. This unchanged full pool is the last legacy insertion.
            if !inline.contains_key(&latest_checkpoint.checkpoint_id) {
                inline.insert(
                    latest_checkpoint.checkpoint_id,
                    inline_pool(latest_checkpoint)?.clone(),
                );
            }
        }
    }
    for checkpoint in &snapshot.checkpoints {
        let inline_pool = inline.remove(&checkpoint.checkpoint_id);
        if inline_pool.is_some()
            || checkpoint
                .context
                .as_ref()
                .is_some_and(|context| remapped.contains(&context.context_version_id))
        {
            plan.checkpoints.push(ResponsesHistoryCheckpointChange {
                checkpoint_id: checkpoint.checkpoint_id,
                before_locator_payload: checkpoint.locator_payload.clone(),
                before_variable_snapshot: checkpoint.variable_snapshot.clone(),
                before_context: checkpoint.context.clone(),
                legacy_inline_variable_pool: inline_pool,
            });
        }
    }
    for continuation in &snapshot.continuations {
        let mut after_payload = continuation.payload.clone();
        replacements.replace_paths(&mut after_payload, CONTINUATION_PROOF)?;
        if after_payload != continuation.payload {
            plan.continuations.push(ResponsesHistoryContinuationChange {
                before: continuation.clone(),
                after_payload,
            });
        }
    }
    Ok(plan)
}

pub async fn preview_verified_flow<R: ResponsesHistoryMigrationRepository + ?Sized>(
    repository: &R,
    snapshot: &ResponsesHistoryFlowSnapshot,
    verified: &[SourcedReplacement],
    master_key: &str,
) -> Result<()> {
    let plan = build_plan(snapshot, verified)?;
    repository
        .preview_responses_history_migration(&plan, master_key)
        .await
}

pub async fn apply_verified_flow<R: ResponsesHistoryMigrationRepository + ?Sized>(
    repository: &R,
    snapshot: &ResponsesHistoryFlowSnapshot,
    verified: &[SourcedReplacement],
    master_key: &str,
) -> Result<ResponsesHistoryMigrationReceipt> {
    let plan = build_plan(snapshot, verified)?;
    repository
        .apply_responses_history_migration(&plan, master_key)
        .await
}

#[cfg(test)]
#[path = "planner/_tests/mod.rs"]
mod tests;

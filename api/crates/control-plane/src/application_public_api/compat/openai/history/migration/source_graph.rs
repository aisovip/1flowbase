//! One-run, application-scoped offline source graph. The host enforces scope and
//! supplies original persisted facts, never previews or reconstructed messages.
use std::collections::BTreeMap;

use anyhow::{ensure, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use domain::orchestration::ResponsesRoundEvidence;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use super::{History, MigrationAccumulator, VerifiedReplacement};

mod native;
use native::NativeGroup;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceNamespace {
    Native,
    Public,
}

/// Compact source identity only; no request or response content is serialized.
#[derive(Debug, Clone, Serialize)]
pub struct SourceLocator {
    pub namespace: SourceNamespace,
    pub flow_run_id: Uuid,
    pub node_run_id: Option<Uuid>,
    pub source_id: Uuid,
    pub sequence: Option<i64>,
    pub completion_source_id: Option<Uuid>,
    pub invocation_id: Option<String>,
    pub provider_attempt_index: Option<u64>,
    pub response_id: Option<String>,
}

#[derive(Serialize)]
pub struct SourcedReplacement {
    pub replacement: VerifiedReplacement,
    pub source: SourceLocator,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnresolvedSource {
    pub source: SourceLocator,
    pub reason: String,
}

#[derive(Serialize)]
pub struct SourceGraphResolution {
    pub verified: Vec<SourcedReplacement>,
    pub unresolved_targets: Vec<Value>,
    pub unresolved_sources: Vec<UnresolvedSource>,
}

pub(super) struct SourceGroup {
    source: SourceLocator,
    body: Value,
    output: Vec<Value>,
    expected_public: Option<Value>,
    failure: Option<String>,
}

type ProofKey = (u32, usize, String);
type States = [MigrationAccumulator; 2];
type ResponseKey = (SourceNamespace, String);

/// Incremental source processing. Finish each native flow after its final source
/// page: resolved bodies/items are released, and response maps retain hashes only.
/// Only forward dependencies retain their raw source group until a later flush.
pub struct HistorySourceGraph {
    targets: BTreeMap<ProofKey, Value>,
    verified: BTreeMap<ProofKey, SourcedReplacement>,
    active_native: BTreeMap<(Uuid, Uuid), NativeGroup>,
    native_sequence: BTreeMap<(Uuid, Uuid), i64>,
    pending: Vec<SourceGroup>,
    responses: BTreeMap<ResponseKey, States>,
    failed: Vec<UnresolvedSource>,
    poisoned: bool,
}

fn parse_proof(value: &Value) -> Result<History> {
    let proof: History = serde_json::from_value(value.clone())
        .map_err(|_| anyhow::anyhow!("native_history_evidence_invalid"))?;
    ensure!(
        proof.digest.len() == 64 && proof.digest.bytes().all(|c| c.is_ascii_hexdigit()),
        "native_history_evidence_invalid"
    );
    ensure!(
        matches!(proof.version, 1 | 2 | 3),
        "native_history_version_unsupported"
    );
    Ok(proof)
}

fn key(history: &History) -> ProofKey {
    (history.version, history.item_count, history.digest.clone())
}

impl HistorySourceGraph {
    pub fn new(targets: &[Value]) -> Result<Self> {
        let mut legacy_targets = BTreeMap::new();
        for target in targets {
            let parsed = parse_proof(target)?;
            if parsed.version != 3 {
                legacy_targets.insert(key(&parsed), target.clone());
            }
        }
        Ok(Self {
            targets: legacy_targets,
            verified: BTreeMap::new(),
            active_native: BTreeMap::new(),
            native_sequence: BTreeMap::new(),
            pending: Vec::new(),
            responses: BTreeMap::new(),
            failed: Vec::new(),
            poisoned: false,
        })
    }

    /// Original runtime_event_original_payload, in increasing sequence per node.
    /// `payload.body` is parsed directly by serde_json, preserving Rust numbers.
    pub fn push_native(
        &mut self,
        flow_run_id: Uuid,
        node_run_id: Option<Uuid>,
        event_id: Uuid,
        sequence: i64,
        event_type: &str,
        payload: &Value,
    ) -> Result<()> {
        ensure!(!self.poisoned, "history_source_graph_poisoned");
        let pending_before = self.pending.len();
        self.feed_native(
            flow_run_id,
            node_run_id,
            event_id,
            sequence,
            event_type,
            payload,
        )?;
        if self.pending.len() != pending_before {
            self.flush()
        } else {
            Ok(())
        }
    }

    /// Close native invocation windows at the actual end of this flow's facts.
    pub fn finish_native_flow(&mut self, flow_run_id: Uuid) -> Result<()> {
        let keys: Vec<_> = self
            .active_native
            .keys()
            .filter(|(flow, _)| *flow == flow_run_id)
            .copied()
            .collect();
        for key in keys {
            if let Some(group) = self.active_native.remove(&key) {
                self.pending.push(group.close());
            }
        }
        self.native_sequence
            .retain(|(flow, _), _| *flow != flow_run_id);
        self.flush()
    }

    /// The host associates this original request frame and persisted public round
    /// with the same flow. Request and round content must reproduce round.history
    /// before any input prefix can contribute a verified replacement.
    pub fn push_public(
        &mut self,
        flow_run_id: Uuid,
        request_source_id: Uuid,
        original_frame: &Value,
        round: &ResponsesRoundEvidence,
    ) -> Result<()> {
        ensure!(!self.poisoned, "history_source_graph_poisoned");
        ensure!(
            original_frame["frame_kind"] == "request",
            "history_source_request_frame_required"
        );
        let encoded = original_frame["body"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("history_source_request_body_missing"))?;
        let bytes = match original_frame["encoding"].as_str() {
            Some("utf8") => encoded.as_bytes().to_vec(),
            Some("base64") => STANDARD.decode(encoded)?,
            _ => anyhow::bail!("history_source_request_encoding_invalid"),
        };
        let body: Value = serde_json::from_slice(&bytes)?;
        ensure!(body.is_object(), "history_source_request_body_invalid");
        ensure!(
            !round.response_id.is_empty(),
            "history_source_response_id_missing"
        );
        parse_proof(&round.history)?;
        self.pending.push(SourceGroup {
            source: SourceLocator {
                namespace: SourceNamespace::Public,
                flow_run_id,
                node_run_id: None,
                source_id: request_source_id,
                sequence: original_frame.get("sequence").and_then(Value::as_i64),
                completion_source_id: None,
                invocation_id: None,
                provider_attempt_index: None,
                response_id: Some(round.response_id.clone()),
            },
            body,
            output: round.output.clone(),
            expected_public: Some(round.history.clone()),
            failure: None,
        });
        self.flush()
    }

    /// Retry unresolved predecessors until no source group makes progress.
    pub fn flush(&mut self) -> Result<()> {
        ensure!(!self.poisoned, "history_source_graph_poisoned");
        loop {
            let mut progress = false;
            for group in std::mem::take(&mut self.pending) {
                match self.resolve_group(&group) {
                    Ok(Some((states, receipts))) => {
                        if let Some(id) = &group.source.response_id {
                            let response_key = (group.source.namespace, id.clone());
                            if let Some(previous) = self.responses.get(&response_key) {
                                if previous.iter().zip(&states).any(|(a, b)| {
                                    a.old.digest != b.old.digest
                                        || a.old.item_count != b.old.item_count
                                        || a.current.digest != b.current.digest
                                }) {
                                    self.poisoned = true;
                                    anyhow::bail!("history_source_response_identity_conflict");
                                }
                            } else {
                                self.responses.insert(response_key, states);
                            }
                        }
                        for (key, receipt) in receipts {
                            if let Some(previous) = self.verified.get(&key) {
                                if previous.replacement.current() != receipt.replacement.current() {
                                    self.poisoned = true;
                                    anyhow::bail!("history_source_replacement_ambiguous");
                                }
                            } else {
                                self.verified.insert(key, receipt);
                            }
                        }
                        // The complete raw group is dropped here. Only hashes and
                        // compact verified locators survive successful resolution.
                        progress = true;
                    }
                    Ok(None) => self.pending.push(group),
                    Err(error) => {
                        self.failed.push(UnresolvedSource {
                            source: group.source,
                            reason: error.to_string(),
                        });
                        progress = true;
                    }
                }
            }
            if !progress {
                break;
            }
        }
        Ok(())
    }

    /// Finalize remaining windows, resolve all known forward dependencies, and
    /// return every target lacking a verified source. No unresolved source is hidden.
    pub fn resolve(mut self) -> Result<SourceGraphResolution> {
        for (_, group) in std::mem::take(&mut self.active_native) {
            self.pending.push(group.close());
        }
        self.flush()?;
        for group in self.pending {
            let predecessor = group
                .body
                .get("previous_response_id")
                .and_then(Value::as_str);
            self.failed.push(UnresolvedSource {
                source: group.source,
                reason: format!(
                    "history_source_predecessor_unresolved:{}",
                    predecessor.unwrap_or_default()
                ),
            });
        }
        let unresolved_targets = self
            .targets
            .into_iter()
            .filter(|(key, _)| !self.verified.contains_key(key))
            .map(|(_, value)| value)
            .collect();
        Ok(SourceGraphResolution {
            verified: self.verified.into_values().collect(),
            unresolved_targets,
            unresolved_sources: self.failed,
        })
    }

    fn resolve_group(
        &self,
        group: &SourceGroup,
    ) -> Result<Option<(States, Vec<(ProofKey, SourcedReplacement)>)>> {
        if let Some(failure) = &group.failure {
            anyhow::bail!("{failure}");
        }
        let mut states = match group.body.get("previous_response_id") {
            None | Some(Value::Null) => {
                [MigrationAccumulator::new(1)?, MigrationAccumulator::new(2)?]
            }
            Some(Value::String(id)) if !id.is_empty() => {
                let Some(previous) = self.responses.get(&(group.source.namespace, id.clone()))
                else {
                    return Ok(None);
                };
                previous.clone()
            }
            _ => anyhow::bail!("history_source_predecessor_invalid"),
        };
        let mut receipts = Vec::new();
        self.match_prefix(&states, &group.source, &mut receipts)?;
        match group.body.get("input") {
            Some(Value::Array(items)) => {
                for item in items { self.append_source(&mut states, item, &group.source, &mut receipts)?; }
            }
            Some(Value::String(text)) => self.append_source(&mut states,
                &serde_json::json!({"type":"message","role":"user","content":[{"type":"input_text","text":text}]}),
                &group.source, &mut receipts)?,
            _ => anyhow::bail!("history_source_input_missing"),
        }
        for item in &group.output {
            self.append_source(&mut states, item, &group.source, &mut receipts)?;
        }
        if let Some(expected) = &group.expected_public {
            let proof = parse_proof(expected)?;
            if proof.version == 3 {
                ensure!(
                    key(&states[0].current) == key(&proof),
                    "history_source_public_round_mismatch"
                );
            } else {
                states[(proof.version - 1) as usize].verify(expected)?;
            }
        }
        Ok(Some((states, receipts)))
    }

    fn append_source(
        &self,
        states: &mut States,
        item: &Value,
        source: &SourceLocator,
        receipts: &mut Vec<(ProofKey, SourcedReplacement)>,
    ) -> Result<()> {
        for state in states.iter_mut() {
            state.append(item)?;
        }
        self.match_prefix(states, source, receipts)
    }

    fn match_prefix(
        &self,
        states: &States,
        source: &SourceLocator,
        receipts: &mut Vec<(ProofKey, SourcedReplacement)>,
    ) -> Result<()> {
        for state in states {
            let key = (
                state.old.version,
                state.old.item_count,
                state.old.digest.clone(),
            );
            if let Some(target) = self.targets.get(&key) {
                receipts.push((
                    key,
                    SourcedReplacement {
                        replacement: state.verify(target)?,
                        source: source.clone(),
                    },
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "source_graph/_tests/mod.rs"]
mod tests;

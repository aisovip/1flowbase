//! Offline maintenance projections. Callers verify protocol proofs; adapters own
//! locking, lossless storage, immutable lineage, and ciphertext integrity.
//! Serializable plans/receipts contain sensitive history and must be kept private.
use async_trait::async_trait;
use domain::FlowRunStatus;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponsesHistoryFlowKey {
    pub scope_id: Uuid,
    pub application_id: Uuid,
    pub flow_run_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsesHistoryFlowPageInput {
    pub scope_id: Uuid,
    pub application_id: Uuid,
    pub after_flow_run_id: Option<Uuid>,
    /// Per-page bound only; follow next_after_flow_run_id until absent.
    pub limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsesHistoryFlowPage {
    pub flows: Vec<ResponsesHistoryFlowKey>,
    pub next_after_flow_run_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsesHistoryNativeSourcePageInput {
    pub flow: ResponsesHistoryFlowKey,
    pub after_sequence: Option<i64>,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponsesHistoryNativeSource {
    pub id: Uuid,
    pub node_run_id: Option<Uuid>,
    pub sequence: i64,
    pub event_type: String,
    /// Restored through runtime_event_original_payload, including archived bodies.
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsesHistoryNativeSourcePage {
    pub sources: Vec<ResponsesHistoryNativeSource>,
    pub next_after_sequence: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResponsesHistoryDocumentOwner {
    FlowInput,
    FlowOutput,
    CallbackRequest { callback_task_id: Uuid },
    NodeOutput { node_run_id: Uuid },
    SpanMetadata { span_id: Uuid },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponsesHistoryDocument {
    pub owner: ResponsesHistoryDocumentOwner,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponsesHistoryDocumentChange {
    pub owner: ResponsesHistoryDocumentOwner,
    pub before: Value,
    pub after: Value,
}

/// Immutable identity only. Materialization belongs to the control plane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponsesHistoryContextIdentity {
    pub context_version_id: Uuid,
    pub actual_content_id: Uuid,
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponsesHistoryContextLineageEntry {
    pub identity: ResponsesHistoryContextIdentity,
    pub previous_projection_id: Option<Uuid>,
    pub sequence: i64,
    pub content: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponsesHistoryCheckpoint {
    pub checkpoint_id: Uuid,
    pub locator_payload: Value,
    pub variable_snapshot: Value,
    /// Actual locator head; content and ancestry occur once in snapshot.contexts.
    /// None identifies a legacy inline checkpoint.
    pub context: Option<ResponsesHistoryContextIdentity>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponsesHistoryCheckpointChange {
    pub checkpoint_id: Uuid,
    pub before_locator_payload: Value,
    pub before_variable_snapshot: Value,
    pub before_context: Option<ResponsesHistoryContextIdentity>,
    /// Some only for an inline checkpoint with no before_context. Its complete
    /// pool is inserted once as runtime_snapshot_v1; existing DAG heads use None.
    pub legacy_inline_variable_pool: Option<Value>,
}

/// Copy one immutable context without expanding its snapshot/delta encoding.
/// Source IDs identify old nodes. The adapter remaps parent IDs using earlier
/// context changes and inserts each old ID at most once.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponsesHistoryContextChange {
    pub before: ResponsesHistoryContextLineageEntry,
    pub after_content: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponsesHistoryContinuation {
    /// Exact persisted key; includes current, response-round and claimed slots.
    pub slot_key: String,
    pub encrypted_payload: Value,
    pub plaintext_digest: String,
    pub plaintext_size_bytes: i64,
    pub key_version: String,
    pub hard_expires_at: OffsetDateTime,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponsesHistoryContinuationChange {
    /// All persisted metadata and authenticated plaintext participate in CAS.
    pub before: ResponsesHistoryContinuation,
    pub after_payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsesHistoryFlowSnapshot {
    pub flow: ResponsesHistoryFlowKey,
    pub status: FlowRunStatus,
    pub latest_context_version_id: Option<Uuid>,
    pub documents: Vec<ResponsesHistoryDocument>,
    /// Unique reachable DAG nodes in topological order. Includes ancestors of
    /// every checkpoint head and latest_context_version_id, loaded once per ID.
    pub contexts: Vec<ResponsesHistoryContextLineageEntry>,
    /// Existing repository order: ascending (created_at, checkpoint_id).
    pub checkpoints: Vec<ResponsesHistoryCheckpoint>,
    /// Includes retained expired slots; loading never invokes runtime expiry cleanup.
    pub continuations: Vec<ResponsesHistoryContinuation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsesHistoryMigrationPlan {
    pub flow: ResponsesHistoryFlowKey,
    pub expected_status: FlowRunStatus,
    pub expected_latest_context_version_id: Option<Uuid>,
    pub documents: Vec<ResponsesHistoryDocumentChange>,
    /// Unique old IDs in topological order. Include unchanged-content children
    /// whenever their parent is remapped. If any context will be inserted,
    /// including a legacy inline context, copy the original latest head last
    /// even if unchanged. Its source ID occurs at most once. The adapter inserts
    /// other nodes and legacy inline contexts before this final head copy.
    pub contexts: Vec<ResponsesHistoryContextChange>,
    /// Preserve snapshot checkpoint order for legacy inline context creation.
    /// If no original latest context exists and any inline context is created,
    /// include the final snapshot checkpoint as an unchanged anchor when needed;
    /// its new context is inserted last, following repository latest selection.
    pub checkpoints: Vec<ResponsesHistoryCheckpointChange>,
    pub continuations: Vec<ResponsesHistoryContinuationChange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponsesHistoryShadowBinding {
    pub shadow_row_id: Uuid,
    pub source_hash: String,
    pub source_byte_size: i64,
    pub canonical_content_id: Uuid,
    pub context_version_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponsesHistoryDocumentReceipt {
    pub change: ResponsesHistoryDocumentChange,
    pub before_shadow: Option<ResponsesHistoryShadowBinding>,
    pub after_shadow: Option<ResponsesHistoryShadowBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponsesHistoryContextReceipt {
    pub before: ResponsesHistoryContextIdentity,
    pub before_previous_projection_id: Option<Uuid>,
    pub before_sequence: i64,
    pub after: ResponsesHistoryContextIdentity,
    pub after_previous_projection_id: Option<Uuid>,
    pub after_sequence: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponsesHistoryCheckpointReceipt {
    pub checkpoint_id: Uuid,
    pub before_locator_payload: Value,
    pub before_variable_snapshot: Value,
    pub before_context: Option<ResponsesHistoryContextIdentity>,
    pub after_locator_payload: Value,
    pub after_variable_snapshot: Value,
    pub after_context: ResponsesHistoryContextIdentity,
    pub after_context_sequence: i64,
    pub before_shadow: Option<ResponsesHistoryShadowBinding>,
    pub after_shadow: Option<ResponsesHistoryShadowBinding>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponsesHistoryContinuationReceipt {
    pub before: ResponsesHistoryContinuation,
    pub after: ResponsesHistoryContinuation,
}

/// Contains exact reversible mutable state. Newly inserted immutable content is
/// deliberately retained by rollback. To restore effective latest recovery data,
/// rollback appends the original latest content with its original parent chain.
/// If no prior context existed, it appends the original latest inline checkpoint
/// pool instead: original context absence/identity cannot be restored. Repeated
/// rollback recognizes that restoration head plus exact mutable preimages.
/// Master keys are never part of this type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsesHistoryMigrationReceipt {
    pub format_version: u32,
    pub flow: ResponsesHistoryFlowKey,
    pub expected_status: FlowRunStatus,
    pub before_latest_context: Option<ResponsesHistoryContextIdentity>,
    pub after_latest_context: Option<ResponsesHistoryContextIdentity>,
    pub documents: Vec<ResponsesHistoryDocumentReceipt>,
    pub contexts: Vec<ResponsesHistoryContextReceipt>,
    pub checkpoints: Vec<ResponsesHistoryCheckpointReceipt>,
    pub continuations: Vec<ResponsesHistoryContinuationReceipt>,
}

/// Read-only source enumeration; no runtime cleanup or business writes.
#[async_trait]
pub trait ResponsesHistoryMigrationReadRepository: Send + Sync {
    async fn list_responses_history_flows(
        &self,
        input: &ResponsesHistoryFlowPageInput,
    ) -> anyhow::Result<ResponsesHistoryFlowPage>;

    async fn list_responses_history_native_sources(
        &self,
        input: &ResponsesHistoryNativeSourcePageInput,
    ) -> anyhow::Result<ResponsesHistoryNativeSourcePage>;
}

#[async_trait]
pub trait ResponsesHistoryMigrationRepository: ResponsesHistoryMigrationReadRepository {
    async fn load_responses_history_flow(
        &self,
        flow: &ResponsesHistoryFlowKey,
        master_key: &str,
    ) -> anyhow::Result<ResponsesHistoryFlowSnapshot>;

    /// Runs the same locks and CAS validation as apply, then rolls back.
    async fn preview_responses_history_migration(
        &self,
        plan: &ResponsesHistoryMigrationPlan,
        master_key: &str,
    ) -> anyhow::Result<()>;

    /// One flow transaction across every document, checkpoint and capsule.
    /// Reapplying an entirely already-after plan must be a no-op.
    async fn apply_responses_history_migration(
        &self,
        plan: &ResponsesHistoryMigrationPlan,
        master_key: &str,
    ) -> anyhow::Result<ResponsesHistoryMigrationReceipt>;

    /// Refuses later changes. Restores effective recovery data by appending a new
    /// latest head; original latest identity/absence is not restored. All source
    /// immutable contexts and audit bindings remain. Repeated rollback is a no-op
    /// only when restoration head and every mutable preimage still match exactly.
    async fn rollback_responses_history_migration(
        &self,
        receipt: &ResponsesHistoryMigrationReceipt,
        master_key: &str,
    ) -> anyhow::Result<()>;
}

#[cfg(test)]
mod _tests;

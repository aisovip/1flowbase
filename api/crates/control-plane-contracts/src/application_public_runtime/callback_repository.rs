use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::ports::{
    FinishFlowRunCallbackResumeAttemptInput, RecordFlowRunCallbackResumeAttemptInput,
    RecordFlowRunCallbackResumeAttemptOutput,
};

#[async_trait]
pub trait ApplicationPublishedCallbackAttemptRepository: Send + Sync {
    async fn record_published_callback_resume_attempt(
        &self,
        input: &RecordFlowRunCallbackResumeAttemptInput,
    ) -> Result<RecordFlowRunCallbackResumeAttemptOutput>;

    async fn get_published_callback_resume_attempt(
        &self,
        callback_task_id: Uuid,
    ) -> Result<Option<domain::FlowRunCallbackResumeAttemptRecord>>;

    async fn finish_published_callback_resume_attempt(
        &self,
        input: &FinishFlowRunCallbackResumeAttemptInput,
    ) -> Result<domain::FlowRunCallbackResumeAttemptRecord>;

    /// `received` → `processing`: exactly one delivery re-acquires a parked
    /// tool-round attempt, and the attempt then carries that delivery's
    /// payload. `None` when the attempt is not parked.
    async fn claim_published_callback_resume_attempt(
        &self,
        attempt_id: Uuid,
        response_payload: Value,
    ) -> Result<Option<domain::FlowRunCallbackResumeAttemptRecord>>;

    /// Whether this callback has a processing execution claim whose lease is live.
    /// Uses the same owner predicate as expired-attempt reclamation.
    async fn has_active_published_callback_resume_claim(
        &self,
        callback_task_id: Uuid,
    ) -> Result<bool>;

    /// Re-admit an identical delivery after its execution claim lease expires.
    async fn reclaim_expired_callback_resume_attempt(
        &self,
        attempt_id: Uuid,
        response_payload: Value,
    ) -> Result<Option<domain::FlowRunCallbackResumeAttemptRecord>>;

    /// Release only this acquisition's processing row version before execution.
    /// Refuse if the version changed, the attempt settled, or an execution claim is live.
    /// Acquisition/re-acquisition must return a strictly increasing row version;
    /// execution-claim acquisition and this check must serialize on the callback owner.
    async fn release_unstarted_published_callback_resume_attempt(
        &self,
        attempt_id: Uuid,
        expected_updated_at: OffsetDateTime,
    ) -> Result<bool>;

    /// `processing` → `received`: the delivery finished without completing the
    /// tool round, so the attempt is parked until further results arrive.
    async fn park_published_callback_resume_attempt(
        &self,
        attempt_id: Uuid,
    ) -> Result<Option<domain::FlowRunCallbackResumeAttemptRecord>>;

    async fn cancel_published_callback_resume_attempts_for_run(
        &self,
        flow_run_id: Uuid,
        completed_at: OffsetDateTime,
    ) -> Result<Vec<domain::FlowRunCallbackResumeAttemptRecord>>;

    async fn fail_waiting_callback_published_run(
        &self,
        flow_run_id: Uuid,
        error_payload: Value,
        finished_at: OffsetDateTime,
    ) -> Result<Option<domain::FlowRunRecord>>;

    async fn complete_waiting_callback_published_internal_run(
        &self,
        flow_run_id: Uuid,
        output_payload: Value,
        finished_at: OffsetDateTime,
    ) -> Result<Option<domain::FlowRunRecord>>;
}

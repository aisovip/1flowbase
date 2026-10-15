use super::*;
use crate::application_public_api::callback_resume::{
    ApplicationPublishedCallbackResumeService, PreparedPublishedCallbackResume,
    PublishedCallbackReservation, PublishedCallbackResumeSource, PublishedCallbackResumeTarget,
    ResumePublishedCallbackCommand,
};

type ReservationService = ApplicationPublishedCallbackResumeService<
    ApplicationPublicApiTestRepository,
    RecordingNativeCallbackConsumer,
>;

async fn reservation_fixture() -> (
    ApplicationPublicApiTestRepository,
    ApplicationApiKeyActor,
    Uuid,
    ResumePublishedCallbackCommand,
    ReservationService,
) {
    let (repository, actor, run) =
        fixture_with_input(json!({"sys":{"requested_model_id":"fixture"}})).await;
    let (callback, body) = seed_proven_full_round(&repository, run, true);
    let (_, payload) = correlate_native_responses_callback(&repository, &actor, &body)
        .await
        .unwrap()
        .unwrap();
    let command = ResumePublishedCallbackCommand {
        responses_continuation: None,
        transport_connection_scope: None,
        observation_context: None,
        reserved_attempt_id: None,
        native_transport: Some(ProviderTransportPayload::openai_responses(body).unwrap()),
        bearer_token: String::new(),
        target: PublishedCallbackResumeTarget::CallbackTask {
            callback_task_id: callback.id,
        },
        source: PublishedCallbackResumeSource::OpenAiResponses,
        response_payload: payload,
        response_mode: Some("streaming".into()),
    };
    let service = ApplicationPublishedCallbackResumeService::new(
        repository.clone(),
        RecordingNativeCallbackConsumer {
            repository: repository.clone(),
            calls: Default::default(),
        },
    );
    (repository, actor, callback.id, command, service)
}

#[tokio::test]
async fn owned_unstarted_release_reacquires_completed_callback_and_fences_old_receipt() {
    let (repository, actor, callback, mut command, service) = reservation_fixture().await;
    let PublishedCallbackReservation::Acquired(first) = service
        .reserve_native_callback_reservation_for_actor(actor.clone(), &command)
        .await
        .unwrap()
    else {
        panic!("first admission must be acquired");
    };
    repository.complete_callback_task_for_test(callback);
    assert!(service
        .release_unstarted_native_callback(&first)
        .await
        .unwrap());
    assert!(!service
        .release_unstarted_native_callback(&first)
        .await
        .unwrap());
    assert!(matches!(
        service
            .prepare_callback_resume_for_actor(actor.clone(), &command)
            .await
            .unwrap(),
        PreparedPublishedCallbackResume::Resume { .. }
    ));
    for source_conflict in [false, true] {
        let mut changed = command.clone();
        if source_conflict {
            changed.source = PublishedCallbackResumeSource::OpenAiChat;
        } else {
            changed.response_payload["tool_results"][0]["content"] = json!("changed");
        }
        assert!(service
            .reserve_native_callback_reservation_for_actor(actor.clone(), &changed)
            .await
            .is_err());
    }
    let PublishedCallbackReservation::Acquired(second) = service
        .reserve_native_callback_reservation_for_actor(actor.clone(), &command)
        .await
        .unwrap()
    else {
        panic!("released completed callback must be acquired immediately");
    };
    assert_eq!(first.attempt_id(), second.attempt_id());
    assert_ne!(first, second);
    assert!(!service
        .release_unstarted_native_callback(&first)
        .await
        .unwrap());
    assert_eq!(
        repository.callback_resume_attempts()[0].status,
        domain::FlowRunCallbackResumeAttemptStatus::Processing
    );
    command.reserved_attempt_id = Some(second.attempt_id());
    service
        .resume_callback_for_actor(actor.clone(), command.clone())
        .await
        .unwrap();
    assert!(!service
        .release_unstarted_native_callback(&second)
        .await
        .unwrap());
    assert!(
        matches!(service.reserve_native_callback_reservation_for_actor(actor, &command).await.unwrap(),
        PublishedCallbackReservation::SettledReplay { attempt_id } if attempt_id == second.attempt_id())
    );
    assert_eq!(repository.callback_resume_attempts().len(), 1);
}

#[tokio::test]
async fn unstarted_release_refuses_new_active_execution_claim() {
    let (repository, actor, callback, command, service) = reservation_fixture().await;
    let PublishedCallbackReservation::Acquired(receipt) = service
        .reserve_native_callback_reservation_for_actor(actor, &command)
        .await
        .unwrap()
    else {
        panic!("first admission must be acquired");
    };
    repository.set_callback_resume_claim_lease_for_test(
        callback,
        OffsetDateTime::now_utc() + time::Duration::minutes(5),
    );
    assert!(!service
        .release_unstarted_native_callback(&receipt)
        .await
        .unwrap());
    assert_eq!(
        repository.callback_resume_attempts()[0].status,
        domain::FlowRunCallbackResumeAttemptStatus::Processing
    );
    repository.set_callback_resume_claim_lease_for_test(
        callback,
        OffsetDateTime::now_utc() - time::Duration::seconds(1),
    );
    assert!(service
        .release_unstarted_native_callback(&receipt)
        .await
        .unwrap());
}

#[tokio::test]
async fn waiting_predecessor_reclassifies_when_aged_execution_owner_expires() {
    let (repository, actor, callback, mut command, service) = reservation_fixture().await;
    service
        .reserve_native_callback_reservation_for_actor(actor.clone(), &command)
        .await
        .unwrap();
    repository.complete_callback_task_for_test(callback);
    repository.expire_callback_resume_attempt_for_test(callback);
    repository.set_callback_resume_claim_lease_for_test(
        callback,
        OffsetDateTime::now_utc() + time::Duration::minutes(5),
    );
    let mut body = command
        .native_transport
        .as_ref()
        .unwrap()
        .wire_body()
        .clone();
    body["input"]
        .as_array_mut()
        .unwrap()
        .push(json!({"role":"user","content":"Continue with new context"}));
    command.native_transport = Some(ProviderTransportPayload::openai_responses(body).unwrap());
    let waiter = tokio::spawn(async move {
        service
            .prepare_callback_resume_for_actor(actor, &command)
            .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    assert!(!waiter.is_finished());
    repository.set_callback_resume_claim_lease_for_test(
        callback,
        OffsetDateTime::now_utc() - time::Duration::seconds(1),
    );
    assert!(matches!(
        tokio::time::timeout(std::time::Duration::from_secs(2), waiter)
            .await
            .expect("expired owner must reclassify without the 240s predecessor timeout")
            .unwrap()
            .unwrap(),
        PreparedPublishedCallbackResume::Resume { .. }
    ));
}

use super::*;

#[test]
fn target_handoff_requires_settlement_and_fences_old_results() {
    let clock = FakeClock::default();
    let mut registry = TransportSessionRegistry::new(clock.clone(), config(1)).unwrap();
    let first = registry.admit(request("upgrade")).unwrap();
    registry.activate(&first).unwrap();
    let lease = registry
        .begin_invocation(&first, invocation_request(None))
        .unwrap();
    let new_target = TransportRuntimeTargetId::new("target-v2").unwrap();
    let before = registry.safe_snapshot();
    assert_eq!(
        registry.rotate_generation_to(&first, new_target.clone()),
        Err(RegistryError::InflightExists)
    );
    assert_eq!(registry.safe_snapshot(), before);
    registry
        .finish_invocation(&lease, InvocationCompletion::IdleAffinity)
        .unwrap();
    registry
        .transition(&first, TransportSessionState::Draining)
        .unwrap();
    let before_release = registry.safe_snapshot();
    assert!(registry
        .rotate_generation_to(&first, new_target.clone())
        .is_err());
    assert_eq!(registry.safe_snapshot(), before_release);
    let evidence = released_evidence(&first, 7);
    registry.record_closure_evidence(&first, &evidence).unwrap();
    let previous = registry.safe_snapshot().sessions[0].clone();
    clock.advance(Duration::from_secs(1));
    let next = registry
        .rotate_generation_to(&first, new_target.clone())
        .unwrap();
    registry.activate(&next).unwrap();
    let current = registry.safe_snapshot().sessions[0].clone();
    assert_eq!(next.session_id, first.session_id);
    assert!(next.generation > first.generation);
    assert_eq!(registry.runtime_target_id(&next).unwrap(), &new_target);
    assert_eq!(current.owner_id, previous.owner_id);
    assert_eq!(current.provider_id, previous.provider_id);
    assert_eq!(
        current.logical_ttl + Duration::from_secs(1),
        previous.logical_ttl
    );
    let successor = registry
        .begin_invocation(&next, invocation_request(None))
        .unwrap();
    assert_eq!(successor.sequence(), lease.sequence() + 1);
    let before_stale = registry.safe_snapshot();
    assert!(matches!(
        registry.record_closure_evidence(&first, &evidence),
        Err(RegistryError::StaleGeneration { .. })
    ));
    assert!(matches!(
        registry.finish_invocation(&lease, InvocationCompletion::IdleAffinity),
        Err(RegistryError::StaleGeneration { .. })
    ));
    assert_eq!(registry.safe_snapshot(), before_stale);
}

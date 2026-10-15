use super::*;
use control_plane::ports::AppendTerminalIfMissingAndCloseOutcome;

fn terminal() -> RuntimeEventPayload {
    RuntimeEventPayload {
        event_type: "flow_finished".into(),
        source: RuntimeEventSource::System,
        durability: RuntimeEventDurability::DurableRequired,
        persist_required: true,
        trace_visible: true,
        payload: json!({ "type": "flow_finished" }),
    }
}

async fn expire_closed(stream: &LocalRuntimeEventStream, run_id: Uuid) {
    let old = OffsetDateTime::now_utc() - TimeDuration::hours(3);
    stream
        .set_run_timestamps_for_tests(run_id, old, Some(old))
        .unwrap();
    wait_for_idle_reclamation(stream, run_id).await;
}

#[tokio::test]
async fn idle_receiver_drop_releases_forwarding_generation_without_an_event() {
    let stream = LocalRuntimeEventStream::new();
    let run_id = Uuid::now_v7();
    stream
        .open_run(run_id, RuntimeEventStreamPolicy::debug_default())
        .await
        .unwrap();
    let probe = stream.run_probe_for_tests(run_id).unwrap();
    let subscription = stream.subscribe(run_id, None).await.unwrap();
    assert_eq!(probe().0, 3, "map, terminal authority, and forwarding task");
    drop(subscription);
    tokio::time::timeout(Duration::from_secs(1), async {
        while probe().0 != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("idle forwarding task must release its run Arc after receiver drop");
}

#[tokio::test]
async fn pinned_expired_terminal_writer_releases_ring_payload_and_allocation() {
    let stream = LocalRuntimeEventStream::new();
    let run_id = Uuid::now_v7();
    stream
        .open_run(run_id, RuntimeEventStreamPolicy::debug_default())
        .await
        .unwrap();
    stream
        .append(
            run_id,
            blob_event(32_000, RuntimeEventDurability::DurableRequired),
        )
        .await
        .unwrap();
    stream
        .append_terminal_if_missing_and_close(run_id, terminal())
        .await
        .unwrap();
    let probe = stream.run_probe_for_tests(run_id).unwrap();
    let subscription = stream.subscribe(run_id, None).await.unwrap();
    let writer = subscription.terminal_writer.clone();
    drop(subscription);
    assert!(probe().1 > 32_000);
    expire_closed(&stream, run_id).await;
    assert_eq!(
        probe(),
        (1, 0, 0),
        "terminal authority may pin metadata, never expired ring bytes"
    );
    assert_eq!(
        writer
            .append_terminal_if_missing_and_close(terminal())
            .await
            .unwrap(),
        AppendTerminalIfMissingAndCloseOutcome::ExistingTerminal
    );
}

#[tokio::test]
async fn expired_old_authority_cannot_close_reopened_generation() {
    let stream = LocalRuntimeEventStream::new();
    let run_id = Uuid::now_v7();
    stream
        .open_run(run_id, RuntimeEventStreamPolicy::debug_default())
        .await
        .unwrap();
    let subscription = stream.subscribe(run_id, None).await.unwrap();
    let old_writer = subscription.terminal_writer.clone();
    drop(subscription);
    stream
        .append_terminal_if_missing_and_close(run_id, terminal())
        .await
        .unwrap();
    expire_closed(&stream, run_id).await;
    stream
        .open_run(run_id, RuntimeEventStreamPolicy::debug_default())
        .await
        .unwrap();
    let current = stream.subscribe(run_id, None).await.unwrap();
    assert_eq!(
        old_writer
            .append_terminal_if_missing_and_close(terminal())
            .await
            .unwrap(),
        AppendTerminalIfMissingAndCloseOutcome::ExistingTerminal
    );
    assert!(current.closure.borrow().is_none());
    assert_eq!(
        stream
            .append(run_id, required_text_delta(1))
            .await
            .unwrap()
            .sequence,
        1
    );
}

#[tokio::test]
async fn expired_closure_without_terminal_remains_an_error_for_old_authority() {
    let stream = LocalRuntimeEventStream::new();
    let run_id = Uuid::now_v7();
    stream
        .open_run(run_id, RuntimeEventStreamPolicy::debug_default())
        .await
        .unwrap();
    let subscription = stream.subscribe(run_id, None).await.unwrap();
    let writer = subscription.terminal_writer.clone();
    drop(subscription);
    stream
        .close_run(run_id, RuntimeEventCloseReason::Finished)
        .await
        .unwrap();
    expire_closed(&stream, run_id).await;
    let error = writer
        .append_terminal_if_missing_and_close(terminal())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("closed without a terminal"));
}

#[tokio::test]
async fn paged_gap_and_capacity_one_slow_reader_deliver_every_event_then_terminal() {
    let stream = LocalRuntimeEventStream::with_broadcast_capacity_for_tests(1);
    let run_id = Uuid::now_v7();
    stream
        .open_run(run_id, RuntimeEventStreamPolicy::debug_default())
        .await
        .unwrap();
    let mut subscription = stream.subscribe(run_id, None).await.unwrap();
    // Hold the consumer beyond several page boundaries and force broadcast lag.
    for index in 0..257 {
        stream
            .append(run_id, required_text_delta(index))
            .await
            .unwrap();
    }
    stream
        .append_terminal_if_missing_and_close(run_id, terminal())
        .await
        .unwrap();
    for sequence in 1..=258 {
        let event = tokio::time::timeout(Duration::from_secs(1), subscription.live_events.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            event.sequence, sequence,
            "paged backfill must not skip or duplicate a sequence"
        );
        if sequence < 258 {
            assert_eq!(event.payload["index"].as_i64(), Some(sequence - 1));
        } else {
            assert_eq!(event.event_type, "flow_finished");
        }
        tokio::task::yield_now().await;
    }
    assert!(subscription.live_events.recv().await.is_none());
}

#[tokio::test]
async fn expiry_preserves_pending_pages_only_for_live_slow_consumers() {
    let stream = LocalRuntimeEventStream::with_broadcast_capacity_for_tests(1);
    let run_id = Uuid::now_v7();
    stream
        .open_run(run_id, RuntimeEventStreamPolicy::debug_default())
        .await
        .unwrap();
    let mut subscription = stream.subscribe(run_id, None).await.unwrap();
    let probe = stream.run_probe_for_tests(run_id).unwrap();
    for index in 0..193 {
        stream
            .append(run_id, required_text_delta(index))
            .await
            .unwrap();
    }
    stream
        .append_terminal_if_missing_and_close(run_id, terminal())
        .await
        .unwrap();
    expire_closed(&stream, run_id).await;
    assert_eq!((probe().1, probe().2), (0, 0));
    for sequence in 1..=194 {
        let event = tokio::time::timeout(Duration::from_secs(1), subscription.live_events.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(event.sequence, sequence);
    }
    assert!(subscription.live_events.recv().await.is_none());
    tokio::time::timeout(Duration::from_secs(1), async {
        while probe().0 != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("only terminal metadata authority may survive the completed consumer");
}

#[tokio::test]
async fn reopening_retires_pinned_old_generation_and_preserves_pending_delivery() {
    let stream = LocalRuntimeEventStream::with_broadcast_capacity_for_tests(1);
    let run_id = Uuid::now_v7();
    stream
        .open_run(run_id, RuntimeEventStreamPolicy::debug_default())
        .await
        .unwrap();
    let mut old = stream.subscribe(run_id, None).await.unwrap();
    let probe = stream.run_probe_for_tests(run_id).unwrap();
    for index in 0..129 {
        stream
            .append(run_id, required_text_delta(index))
            .await
            .unwrap();
    }
    stream
        .append_terminal_if_missing_and_close(run_id, terminal())
        .await
        .unwrap();
    stream
        .open_run(run_id, RuntimeEventStreamPolicy::debug_default())
        .await
        .unwrap();
    assert_eq!(
        (probe().1, probe().2),
        (0, 0),
        "replaced generation cannot retain an unscheduled ring"
    );
    let current = stream.subscribe(run_id, None).await.unwrap();
    assert_eq!(
        old.terminal_writer
            .append_terminal_if_missing_and_close(terminal())
            .await
            .unwrap(),
        AppendTerminalIfMissingAndCloseOutcome::ExistingTerminal
    );
    assert!(current.closure.borrow().is_none());
    assert_eq!(
        stream
            .append(run_id, required_text_delta(500))
            .await
            .unwrap()
            .sequence,
        1
    );
    for sequence in 1..=130 {
        let event = tokio::time::timeout(Duration::from_secs(1), old.live_events.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(event.sequence, sequence);
    }
    assert!(old.live_events.recv().await.is_none());
}

#[tokio::test]
async fn captured_generation_close_cannot_close_reopened_run_or_create_terminal() {
    let stream = LocalRuntimeEventStream::new();
    let run_id = Uuid::now_v7();
    stream
        .open_run(run_id, RuntimeEventStreamPolicy::debug_default())
        .await
        .unwrap();
    let old = stream.terminal_writer(run_id).await.unwrap();
    let old_generation = old.generation_id();
    old.close_generation(RuntimeEventCloseReason::Failed)
        .await
        .unwrap();
    assert_eq!(
        old.closure().unwrap().reason,
        RuntimeEventCloseReason::Failed
    );
    assert_eq!(old.closure().unwrap().final_sequence, 0);
    assert!(
        stream.replay(run_id, None, 10).await.unwrap().is_empty(),
        "close-only cleanup must not fabricate a terminal event"
    );

    stream
        .open_run(run_id, RuntimeEventStreamPolicy::debug_default())
        .await
        .unwrap();
    let new = stream.terminal_writer(run_id).await.unwrap();
    assert_ne!(old_generation, new.generation_id());
    stream.append(run_id, heartbeat()).await.unwrap();
    old.close_generation(RuntimeEventCloseReason::Cancelled)
        .await
        .unwrap();
    assert!(
        new.closure().is_none(),
        "old authority must never resolve a new generation by run ID"
    );
    assert_eq!(
        old.closure().unwrap().reason,
        RuntimeEventCloseReason::Failed
    );
    stream.append(run_id, heartbeat()).await.unwrap();
    assert_eq!(stream.replay(run_id, None, 10).await.unwrap().len(), 2);
    new.close_generation(RuntimeEventCloseReason::Cancelled)
        .await
        .unwrap();
    assert_eq!(
        new.closure().unwrap().reason,
        RuntimeEventCloseReason::Cancelled
    );
    assert_eq!(new.closure().unwrap().final_sequence, 2);
}

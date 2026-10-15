use super::*;
use event_forwarding::is_public_terminal_runtime_event;

pub(super) async fn attach_live_compatible_callback_round(
    dependencies: &CompatibilityExecutionDependencies,
    initial_run: NativeRunResult,
    round_id: uuid::Uuid,
    start: i64,
) -> Result<CompatibleTypedTurnStream, NativeApiError> {
    let subscription = dependencies
        .native
        .runtime_event_stream
        .subscribe(initial_run.id, Some(start))
        .await
        .map_err(service_error)?;
    let subscription = follow_callback_subscription(
        dependencies.clone(),
        initial_run.clone(),
        round_id,
        start,
        subscription,
    );
    Ok(attach_compatible_typed_stream_from_subscription(
        native::native_run_terminal_dependencies(&dependencies.native),
        initial_run,
        Some(start),
        Vec::new(),
        subscription,
    ))
}

/// A callback producer can finish durably after its original transport consumer
/// exits. Keep live delivery, but do not mistake an open ephemeral lane for the
/// owner of the callback's committed outcome.
pub(super) fn follow_callback_subscription(
    dependencies: CompatibilityExecutionDependencies,
    initial_run: NativeRunResult,
    round_id: uuid::Uuid,
    from_sequence: i64,
    mut subscription: control_plane::ports::RuntimeEventSubscription,
) -> control_plane::ports::RuntimeEventSubscription {
    let (sender, receiver) = mpsc::channel(32);
    let (diagnostic_sender, diagnostic) = mpsc::channel(1);
    drop(diagnostic_sender);
    let replay = std::mem::take(&mut subscription.replay);
    let terminal_writer = subscription.terminal_writer;
    let closure = subscription.closure;
    let mut cursor = replay.last().map_or(from_sequence, |event| event.sequence);
    let mut started = replay
        .iter()
        .any(|event| event.event_type == "flow_started");
    let mut completed_items = std::collections::HashSet::new();
    for event in &replay {
        remember_completed_output(event, &mut completed_items);
    }
    tokio::spawn(async move {
        let mut durable_cursor = match dependencies
            .native
            .store
            .get_runtime_event_sequence_for_callback_task(initial_run.id, round_id)
            .await
        {
            Ok(Some(boundary)) => boundary,
            result => {
                warn!(flow_run_id = %initial_run.id, ?result,
                    "callback attachment durable boundary unavailable");
                return;
            }
        };
        let mut terminal_observed = false;
        let mut live_open = true;
        let mut poll = tokio::time::interval(Duration::from_secs(1));
        poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = sender.closed() => return,
                event = subscription.live_events.recv(), if live_open => {
                    let Some(event) = event else {
                        live_open = false;
                        continue;
                    };
                    cursor = cursor.max(event.sequence);
                    started |= event.event_type == "flow_started";
                    remember_completed_output(&event, &mut completed_items);
                    let terminal = is_public_terminal_runtime_event(&event.event_type);
                    if sender.send(event).await.is_err() || terminal {
                        return;
                    }
                }
                _ = poll.tick() => {
                    while !terminal_observed {
                        let page = match dependencies.native.store.list_runtime_event_durable_page(
                            initial_run.id, durable_cursor, None, 64,
                        ).await {
                            Ok(page) => page,
                            Err(error) => {
                                warn!(flow_run_id = %initial_run.id, %error,
                                    "failed to probe durable callback completion");
                                break;
                            }
                        };
                        if let Some(last) = page.last() {
                            if last.sequence <= durable_cursor {
                                warn!(flow_run_id = %initial_run.id,
                                    "durable callback attachment cursor did not advance");
                                return;
                            }
                            durable_cursor = last.sequence;
                        }
                        terminal_observed |= page.iter().any(|event|
                            is_public_terminal_runtime_event(&event.event_type));
                        if page.len() < 64 || terminal_observed {
                            break;
                        }
                    }
                    if !terminal_observed {
                        continue;
                    }
                    let replay = match durable_compatible_round_replay(
                        &dependencies, &initial_run, round_id,
                    ).await {
                        Ok(DurableCompatibleRoundReplay::Ready(replay)) if replay.last()
                            .is_some_and(|event| is_public_terminal_runtime_event(&event.event_type)) => replay,
                        Ok(DurableCompatibleRoundReplay::PendingIdentity) => continue,
                        Ok(DurableCompatibleRoundReplay::Ready(_)) => {
                            // A resolved identity can exclude a foreign terminal.
                            terminal_observed = false;
                            continue;
                        }
                        Err(error) => {
                            warn!(flow_run_id = %initial_run.id, ?error,
                                "failed to follow durable callback completion");
                            continue;
                        }
                    };
                    let live_cursor = cursor;
                    for mut event in replay {
                        let local_sequence = event.payload["sequence_end"].as_i64()
                            .or_else(|| event.payload["stream_sequence"].as_i64());
                        let item_index = completed_output_index(&event);
                        if !is_public_terminal_runtime_event(&event.event_type)
                            && (local_sequence.is_some_and(|sequence| sequence <= live_cursor)
                                || (event.event_type == "flow_started" && started)
                                || item_index.is_some_and(|index| completed_items.contains(&index))) {
                            continue;
                        }
                        // Durable and live cursors belong to different sequence spaces.
                        // This bridge appends the missing suffix to its live lane.
                        cursor = cursor.saturating_add(1);
                        event.sequence = cursor;
                        event.event_id = format!("{}:{}", event.run_id, cursor);
                        if sender.send(event).await.is_err() {
                            return;
                        }
                    }
                    return;
                }
            }
        }
    });
    control_plane::ports::RuntimeEventSubscription {
        terminal_writer,
        replay,
        live_events: control_plane::ports::RuntimeEventReceiver::new(receiver, diagnostic),
        closure,
    }
}

fn completed_output_index(event: &RuntimeEventEnvelope) -> Option<u64> {
    (event.event_type == "provider_output_item_done")
        .then(|| event.payload["output_index"].as_u64())
        .flatten()
}

fn remember_completed_output(
    event: &RuntimeEventEnvelope,
    completed_items: &mut std::collections::HashSet<u64>,
) {
    if let Some(index) = completed_output_index(event) {
        completed_items.insert(index);
    }
}

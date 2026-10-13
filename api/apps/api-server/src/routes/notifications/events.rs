use crate::{app_state::ApiState, error_response::ApiError};
use axum::{
    extract::State,
    http::HeaderMap,
    response::sse::{Event, KeepAlive, Sse},
};
use std::{
    collections::HashMap,
    convert::Infallible,
    sync::{Arc, Mutex},
};
use tokio::sync::broadcast;
use uuid::Uuid;

#[derive(Default)]
pub struct NotificationEventHub {
    channels: Mutex<HashMap<(Uuid, Uuid), broadcast::Sender<()>>>,
}
impl NotificationEventHub {
    fn subscribe(&self, workspace: Uuid, user: Uuid) -> broadcast::Receiver<()> {
        self.channels
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry((workspace, user))
            .or_insert_with(|| broadcast::channel(16).0)
            .subscribe()
    }
}
impl control_plane::notifications::NotificationEventPublisher for NotificationEventHub {
    fn publish(&self, workspace_id: Uuid, users: &[Uuid]) {
        let channels = self
            .channels
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for user in users {
            if let Some(sender) = channels.get(&(workspace_id, *user)) {
                let _ = sender.send(());
            }
        }
    }
}
// Protocol control: this transport carries only an inbox-refresh signal and no business payload.
pub async fn events(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let context = crate::middleware::require_session::require_session(&state, &headers).await?;
    let receiver = state
        .notification_events
        .subscribe(context.actor.current_workspace_id, context.actor.user_id);
    let stream =
        futures_util::stream::unfold((receiver, true), |(mut receiver, initial)| async move {
            if !initial {
                match receiver.recv().await {
                    Ok(()) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return None,
                }
            }
            Some((
                Ok::<_, Infallible>(Event::default().event("notification.changed").data("{}")),
                (receiver, false),
            ))
        });
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

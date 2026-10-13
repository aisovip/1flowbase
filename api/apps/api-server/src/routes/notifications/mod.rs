use crate::{
    app_state::ApiState,
    error_response::ApiError,
    response::ApiSuccess,
    routes::{
        console_interface,
        console_route_assembly::{console_get, console_post, ConsoleRouteAssembly},
    },
};
use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    Json,
};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;
pub mod events;
pub(crate) mod interface;
use interface::{NotificationInput as Input, NotificationOutput};

pub fn route_assembly() -> ConsoleRouteAssembly<Arc<ApiState>> {
    use access_control::ConsoleRouteOwnership::ConsoleOperation as Op;
    ConsoleRouteAssembly::new()
        .route(
            "/notifications/types",
            console_get(types, Op("notifications.types.list".into())),
        )
        .route(
            "/notifications/instances",
            console_get(list, Op("notifications.instances.list".into()))
                .post(create, Op("notifications.instances.create".into())),
        )
        .route(
            "/notifications/instances/:instance_id",
            console_get(get, Op("notifications.instances.get".into()))
                .patch(patch, Op("notifications.instances.update".into()))
                .delete(delete, Op("notifications.instances.delete".into())),
        )
        .route(
            "/notifications/instances/:instance_id/send",
            console_post(send, Op("notifications.instances.send".into())),
        )
        .route(
            "/notifications/messages",
            console_get(messages, Op("notifications.messages.list".into())),
        )
        .route(
            "/notifications/messages/unread-count",
            console_get(unread, Op("notifications.messages.unread_count".into())),
        )
        .route(
            "/notifications/messages/:notification_id/read",
            console_post(read, Op("notifications.messages.read".into())),
        )
        .route(
            "/notifications/messages/read-all",
            console_post(read_all, Op("notifications.messages.read_all".into())),
        )
        .route(
            "/notifications/events",
            console_get(events::events, Op("notifications.events.subscribe".into())),
        )
}
async fn invoke(
    state: Arc<ApiState>,
    headers: HeaderMap,
    operation: &str,
    input: Input,
) -> Result<Json<ApiSuccess<Value>>, ApiError> {
    let snapshot_state = Arc::clone(&state);
    let credential = match &input {
        Input::Types | Input::List | Input::Get(_) | Input::Messages(_) | Input::UnreadCount => {
            crate::extension_bus::ConsoleAuthenticationCredential::Protocol { state, headers }
        }
        Input::Create(_)
        | Input::Patch(..)
        | Input::Delete(_)
        | Input::Send(..)
        | Input::Read(_)
        | Input::ReadAll => {
            crate::extension_bus::ConsoleAuthenticationCredential::ProtocolWithCsrf {
                state,
                headers,
            }
        }
    };
    let NotificationOutput(value) = console_interface::invoke(
        snapshot_state,
        &format!("http.console.notifications.{operation}.v1"),
        credential,
        input,
    )
    .await?;
    Ok(Json(ApiSuccess::new(value)))
}
pub async fn types(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
) -> Result<Json<ApiSuccess<Value>>, ApiError> {
    invoke(state, headers, "types.list", Input::Types).await
}
pub async fn list(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
) -> Result<Json<ApiSuccess<Value>>, ApiError> {
    invoke(state, headers, "instances.list", Input::List).await
}
pub async fn unread(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
) -> Result<Json<ApiSuccess<Value>>, ApiError> {
    invoke(state, headers, "messages.unread_count", Input::UnreadCount).await
}
pub async fn read_all(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
) -> Result<Json<ApiSuccess<Value>>, ApiError> {
    invoke(state, headers, "messages.read_all", Input::ReadAll).await
}
pub async fn get(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiSuccess<Value>>, ApiError> {
    invoke(state, headers, "instances.get", Input::Get(id)).await
}
pub async fn delete(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiSuccess<Value>>, ApiError> {
    invoke(state, headers, "instances.delete", Input::Delete(id)).await
}
pub async fn read(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiSuccess<Value>>, ApiError> {
    invoke(state, headers, "messages.read", Input::Read(id)).await
}

pub async fn create(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Json(body): Json<control_plane::notifications::CreateNotificationInstance>,
) -> Result<Json<ApiSuccess<Value>>, ApiError> {
    invoke(state, headers, "instances.create", Input::Create(body)).await
}
pub async fn patch(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<control_plane::notifications::PatchNotificationInstance>,
) -> Result<Json<ApiSuccess<Value>>, ApiError> {
    invoke(state, headers, "instances.update", Input::Patch(id, body)).await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SendBody {
    pub input: Value,
}
pub async fn send(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<SendBody>,
) -> Result<Json<ApiSuccess<Value>>, ApiError> {
    invoke(
        state,
        headers,
        "instances.send",
        Input::Send(id, body.input),
    )
    .await
}
#[derive(Deserialize, Default)]
pub struct MessageQuery {
    pub cursor: Option<String>,
    pub page_size: Option<u32>,
    pub unread_only: Option<bool>,
}
pub async fn messages(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Query(query): Query<MessageQuery>,
) -> Result<Json<ApiSuccess<Value>>, ApiError> {
    invoke(
        state,
        headers,
        "messages.list",
        Input::Messages(control_plane::ports::NotificationMessageListInput {
            cursor: query.cursor,
            page_size: query.page_size,
            unread_only: query.unread_only.unwrap_or(false),
        }),
    )
    .await
}

//! API composition adapter for the resource replay + file upload Host Service.
use crate::{app_state::ApiState, error_response::ApiError};
use extension_contracts::{FileImportError, FileImportRequest};
use runtime_core::runtime_backend::{
    FileImportBinding, FileImportFuture, FileImportPort, RuntimeCallerContext,
};
use std::{
    future::Future,
    sync::{Arc, OnceLock, Weak},
};

tokio::task_local! {
    static SELECTED_MCP_INSTANCES: Vec<String>;
    static RESOURCE_IMPORT_REPLAY: bool;
}
pub(crate) fn selected_mcp_scope() -> Option<Vec<String>> {
    SELECTED_MCP_INSTANCES.try_with(Clone::clone).ok()
}
fn importing_from_replay() -> bool {
    RESOURCE_IMPORT_REPLAY
        .try_with(|replay| *replay)
        .unwrap_or(false)
}
pub(crate) async fn with_selected_scope<F: Future>(ids: Vec<String>, future: F) -> F::Output {
    SELECTED_MCP_INSTANCES.scope(ids, future).await
}
pub(crate) async fn with_replay<F: Future>(future: F) -> F::Output {
    RESOURCE_IMPORT_REPLAY.scope(true, future).await
}

/// Called before any worker task is spawned. Task locals are never expected to cross a spawn.
pub(crate) async fn capture_caller(
    state: &ApiState,
    principal: &interface_runtime::UserPrincipal,
) -> Result<Option<RuntimeCallerContext>, ApiError> {
    if importing_from_replay() {
        return Ok(None);
    }
    let actor = principal.actor().clone();
    let user = state
        .store
        .find_user_by_id(actor.user_id)
        .await?
        .ok_or(control_plane::errors::ControlPlaneError::NotAuthenticated)?;
    if matches!(user.status, domain::UserStatus::Disabled) {
        return Err(control_plane::errors::ControlPlaneError::NotAuthenticated.into());
    }
    let session_version = if let Some(delegated) =
        crate::middleware::require_session::server_delegated_request_context()
    {
        if delegated.actor != actor {
            return Err(control_plane::errors::ControlPlaneError::NotAuthenticated.into());
        }
        delegated.user.session_version
    } else if let Some(session) = principal.authenticated_session() {
        state
            .session_store
            .get(session.expose_to_trusted_handler())
            .await?
            .filter(|session| {
                session.user_id == actor.user_id
                    && session.current_workspace_id == actor.current_workspace_id
                    && session.tenant_id == actor.tenant_id
            })
            .ok_or(control_plane::errors::ControlPlaneError::NotAuthenticated)?
            .session_version
    } else {
        user.session_version
    };
    if session_version != user.session_version {
        return Err(control_plane::errors::ControlPlaneError::NotAuthenticated.into());
    }
    Ok(Some(RuntimeCallerContext {
        actor,
        session_version,
        selected_mcp_instance_ids: selected_mcp_scope(),
    }))
}

/// Weak publisher binding follows the current generation without retaining old state or routers.
#[derive(Default)]
pub(crate) struct ApiFileImportPort {
    publisher: OnceLock<Weak<crate::managed_publication::ManagedGenerationPublisher>>,
}
impl ApiFileImportPort {
    pub(crate) fn bind(
        &self,
        publisher: &Arc<crate::managed_publication::ManagedGenerationPublisher>,
    ) -> anyhow::Result<()> {
        self.publisher
            .set(Arc::downgrade(publisher))
            .map_err(|_| anyhow::anyhow!("file import host already bound"))
    }
    fn current_state(&self) -> Option<Arc<ApiState>> {
        self.publisher.get()?.upgrade()?.current_state()
    }
    async fn current_caller(
        &self,
        state: &ApiState,
        binding: &FileImportBinding,
    ) -> Result<(domain::ActorContext, domain::UserRecord), FileImportError> {
        let denied = || FileImportError::new("file_import_caller_invalid");
        let delegated = &binding.caller;
        let user = state
            .store
            .find_user_by_id(delegated.actor.user_id)
            .await
            .map_err(|_| denied())?
            .ok_or_else(denied)?;
        if user.session_version != delegated.session_version
            || matches!(user.status, domain::UserStatus::Disabled)
        {
            return Err(denied());
        }
        let actor = state
            .store
            .load_actor_context(
                user.id,
                delegated.actor.tenant_id,
                delegated.actor.current_workspace_id,
                Some(&delegated.actor.effective_display_role),
            )
            .await
            .map_err(|_| denied())?;
        if actor.effective_display_role != delegated.actor.effective_display_role {
            return Err(denied());
        }
        Ok((actor, user))
    }
}
impl FileImportPort for ApiFileImportPort {
    fn execute<'a>(
        &'a self,
        binding: &'a FileImportBinding,
        request: &'a FileImportRequest,
    ) -> FileImportFuture<'a> {
        Box::pin(async move {
            request.validate()?;
            let state = self
                .current_state()
                .ok_or_else(|| FileImportError::new("file_import_host_unavailable"))?;
            let (actor, user) = self.current_caller(&state, binding).await?;
            // Check the explicit opt-in before replay, but do not hold the same authority lock
            // while replay re-enters canonical admission. Target effects reacquire the lease.
            let lease =
                control_plane::file_management::authorize_resource_import(&state.store, binding)
                    .await?;
            lease
                .release()
                .await
                .map_err(|_| FileImportError::new("file_import_authority_unavailable"))?;
            let source = crate::routes::mcp_protocol::resources::replay_for_import(
                &state,
                binding,
                &actor,
                user,
                &request.resource_uri,
            )
            .await?;
            let (actor, _) = self.current_caller(&state, binding).await?;
            let upload = control_plane::file_management::FileUploadService::new(
                state.store.clone(),
                state.file_storage_registry.clone(),
                state.runtime_engine.clone(),
            );
            control_plane::file_management::ResourceImportService::new(
                upload,
                Arc::new(state.store.clone()),
            )
            .import(
                binding,
                actor,
                uuid::Uuid::parse_str(&request.file_table_id)
                    .map_err(|_| FileImportError::new("file_import_request_invalid"))?,
                source,
            )
            .await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn trusted_scope_capture_distinguishes_direct_console_and_empty_selection_and_blocks_recursive_import(
    ) {
        assert!(selected_mcp_scope().is_none());
        with_selected_scope(vec![], async {
            assert_eq!(selected_mcp_scope(), Some(vec![]));
        })
        .await;
        with_selected_scope(vec!["selected".into()], async {
            let captured = selected_mcp_scope();
            assert_eq!(captured, Some(vec!["selected".into()]));
            assert!(!importing_from_replay());
            with_replay(async {
                assert!(importing_from_replay());
                with_replay(async {
                    assert!(importing_from_replay());
                })
                .await;
                assert!(importing_from_replay());
            })
            .await;
            assert!(!importing_from_replay());
            // Spawned callbacks must receive the captured Host value, not guess from their URI.
            tokio::spawn(async move {
                assert_eq!(captured, Some(vec!["selected".into()]));
                assert!(selected_mcp_scope().is_none());
            })
            .await
            .unwrap();
        })
        .await;
        assert!(selected_mcp_scope().is_none());
    }
}

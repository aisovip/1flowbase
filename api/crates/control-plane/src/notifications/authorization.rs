//! Host-owned authorization state, encrypted write set and optimistic attempt fencing.
use super::*;
use crate::ports::{NotificationAuthorizationSnapshot, SaveNotificationAuthorizationInput};
use extension_contracts::{
    NotificationAuthorizationOperation as Operation, NotificationAuthorizationStatus as Status,
    NotificationOperation, NotificationRuntimeInput, NotificationRuntimeResult,
};
use std::collections::BTreeMap;

pub(super) fn unauthorized() -> NotificationAuthorizationView {
    NotificationAuthorizationView {
        status: Status::Unauthorized,
        user_action: None,
    }
}

impl<R> NotificationService<R>
where
    R: NotificationRepository + PluginRepository + RoleConsolePolicyReader,
{
    /// Authority and workspace are checked before reading credentials or dispatching a worker.
    pub async fn authorize_instance(
        &self,
        actor: &ActorContext,
        instance_id: Uuid,
        operation: Operation,
    ) -> Result<NotificationAuthorizationView> {
        self.manage(actor, "notifications.instances.authorization")
            .await?;
        let record = self.record(actor, instance_id).await?;
        let entry = self
            .type_entry(actor, &record.type_code, record.installation_id)
            .await?;
        if entry.descriptor.authorization.is_none() {
            return Err(Error::InvalidInput("notification_authorization_unsupported").into());
        }
        let mut state = self
            .repository
            .resolve_notification_authorization(
                actor.current_workspace_id,
                instance_id,
                &self.master_key,
            )
            .await?;
        let current = state.authorization.clone().unwrap_or_else(unauthorized);
        let original_secrets = state.managed_secrets.clone();
        let mut input = NotificationRuntimeInput {
            operation: NotificationOperation::Authorize {
                operation: operation.clone(),
            },
            managed_secrets: state.managed_secrets.clone(),
        };
        input
            .validate(&entry.descriptor)
            .map_err(|_| Error::InvalidInput("notification_authorization_operation"))?;

        if matches!(operation, Operation::Cancel) {
            // Cancellation is durable before optional supplier cleanup. Any in-flight begin/poll
            // now has a stale revision; cleanup response is never permitted to resurrect it.
            self.persist_authorization(
                actor,
                instance_id,
                state.revision,
                unauthorized(),
                BTreeMap::new(),
            )
            .await?
            .ok_or(Error::Conflict("notification_authorization_stale"))?;
            if entry.available {
                if let (Some(provider), Some(installation_id)) =
                    (&self.provider, record.installation_id)
                {
                    if let Ok(config) = self
                        .notification_config(actor, &record, &entry.descriptor)
                        .await
                    {
                        let _ = provider
                            .send(
                                actor,
                                installation_id,
                                &entry.descriptor,
                                config,
                                serde_json::to_value(input)?,
                            )
                            .await;
                    }
                }
            }
            return Ok(unauthorized());
        }
        if matches!(operation, Operation::Poll) && current.status != Status::Pending {
            return Ok(current);
        }
        if !entry.available {
            return Err(Error::Conflict("plugin_unavailable").into());
        }
        let config = self
            .notification_config(actor, &record, &entry.descriptor)
            .await?;
        if matches!(operation, Operation::Begin { .. }) {
            // Claim the new attempt before network I/O. A failed begin leaves an unauthorized
            // instance rather than exposing old QR credentials or accepting a late old poll.
            state.revision = self
                .persist_authorization(
                    actor,
                    instance_id,
                    state.revision,
                    unauthorized(),
                    BTreeMap::new(),
                )
                .await?
                .ok_or(Error::Conflict("notification_authorization_stale"))?;
            state.authorization = Some(unauthorized());
            state.managed_secrets.clear();
            input.managed_secrets.clear();
        }
        let raw = self
            .provider
            .as_ref()
            .ok_or(Error::Conflict("plugin_unavailable"))?
            .send(
                actor,
                record
                    .installation_id
                    .ok_or(Error::Conflict("type_unavailable"))?,
                &entry.descriptor,
                config.clone(),
                serde_json::to_value(&input)?,
            )
            .await
            .map_err(|_| Error::UpstreamUnavailable("notification_authorization_failed"))?;
        let response: NotificationRuntimeResult = serde_json::from_value(raw)
            .map_err(|_| Error::UpstreamUnavailable("notification_authorization_failed"))?;
        response
            .validate_for(&input, &entry.descriptor)
            .map_err(|_| Error::UpstreamUnavailable("notification_authorization_failed"))?;
        let NotificationRuntimeResult::Authorization {
            authorization,
            managed_secret_patch,
        } = response
        else {
            return Err(Error::UpstreamUnavailable("notification_authorization_failed").into());
        };
        // Neither QR display nor any optional public field may reflect stored/new credentials.
        let public = serde_json::to_value(&authorization)?;
        if public_contains_credentials(
            &public,
            &entry.descriptor,
            &config,
            &original_secrets,
            &managed_secret_patch,
        ) {
            return Err(Error::UpstreamUnavailable("notification_authorization_failed").into());
        }
        apply_patch(&mut state.managed_secrets, managed_secret_patch);
        self.persist_authorization(
            actor,
            instance_id,
            state.revision,
            authorization.clone(),
            state.managed_secrets,
        )
        .await?
        .ok_or(Error::Conflict("notification_authorization_stale"))?;
        Ok(authorization)
    }

    pub(super) async fn notification_config(
        &self,
        actor: &ActorContext,
        record: &NotificationInstanceRecord,
        descriptor: &NotificationTypeDescriptor,
    ) -> Result<Value> {
        let mut config = record
            .config_json
            .as_object()
            .cloned()
            .ok_or(Error::Conflict("notification_config"))?;
        let secret = self
            .repository
            .resolve_notification_secret_config(
                actor.current_workspace_id,
                record.id,
                &self.master_key,
            )
            .await?;
        config.extend(
            secret
                .as_object()
                .cloned()
                .ok_or(Error::Conflict("notification_config"))?,
        );
        let config = Value::Object(config);
        descriptor
            .validate_config(&config)
            .map_err(|_| Error::InvalidInput("notification_config_schema"))?;
        Ok(config)
    }

    pub(super) async fn persist_authorization(
        &self,
        actor: &ActorContext,
        instance_id: Uuid,
        expected_revision: i64,
        authorization: NotificationAuthorizationView,
        managed_secrets: BTreeMap<String, Value>,
    ) -> Result<Option<i64>> {
        self.repository
            .save_notification_authorization(SaveNotificationAuthorizationInput {
                workspace_id: actor.current_workspace_id,
                instance_id,
                expected_revision,
                authorization,
                managed_secrets,
                master_key: self.master_key.clone(),
            })
            .await
    }

    pub(super) async fn apply_runtime_authorization(
        &self,
        actor: &ActorContext,
        instance_id: Uuid,
        mut state: NotificationAuthorizationSnapshot,
        authorization: Option<NotificationAuthorizationView>,
        patch: BTreeMap<String, Value>,
    ) -> Result<()> {
        if authorization.is_none() && patch.is_empty() {
            return Ok(());
        }
        let view = authorization
            .or(state.authorization)
            .unwrap_or_else(unauthorized);
        apply_patch(&mut state.managed_secrets, patch);
        // A send's external outcome stands even if cancel/re-begin superseded its state. Drop
        // the old write set instead of replacing newer credentials or converting delivery.
        let _ = self
            .persist_authorization(
                actor,
                instance_id,
                state.revision,
                view,
                state.managed_secrets,
            )
            .await?;
        Ok(())
    }
}

pub(super) fn apply_patch(secrets: &mut BTreeMap<String, Value>, patch: BTreeMap<String, Value>) {
    for (key, value) in patch {
        if value.is_null() {
            secrets.remove(&key);
        } else {
            secrets.insert(key, value);
        }
    }
}

pub(super) fn public_contains_credentials(
    public: &Value,
    descriptor: &NotificationTypeDescriptor,
    config: &Value,
    managed: &BTreeMap<String, Value>,
    patch: &BTreeMap<String, Value>,
) -> bool {
    fn reflects(public: &Value, secret: &Value) -> bool {
        match secret {
            Value::String(value) => !value.is_empty() && contains_secret(public, value),
            Value::Array(values) => values.iter().any(|value| reflects(public, value)),
            Value::Object(fields) => fields.values().any(|value| reflects(public, value)),
            _ => false,
        }
    }
    descriptor
        .secret_config_fields()
        .filter_map(|key| config.get(key))
        .any(|secret| reflects(public, secret))
        || managed.iter().chain(patch.iter()).any(|(key, secret)| {
            if key != "qr_code_url" && key != "qr_credential" {
                return reflects(public, secret);
            }
            // A QR URL intentionally encodes its challenge. Only the exact cached URL
            // in the authorization display field may reveal qr_credential/qr_code_url;
            // bot tokens and every other secret remain protected even inside that URL.
            let display_url = patch
                .get("qr_code_url")
                .or_else(|| managed.get("qr_code_url"));
            let mut projected = public.clone();
            for action in ["/user_action", "/authorization/user_action"] {
                if let Some(action) = projected.pointer_mut(action) {
                    if display_url.is_some() && action.get("qr_code_url") == display_url {
                        action
                            .as_object_mut()
                            .expect("QR action object")
                            .remove("qr_code_url");
                    }
                }
            }
            reflects(&projected, secret)
        })
}

#[cfg(test)]
#[path = "_tests/authorization.rs"]
mod _tests;

//! Opt-in notification operations inside Capability Execute's input_payload/result.
//! Host encrypts managed secrets independently of config and never projects patches to clients.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{identity, invalid, ContractResult, NotificationTypeDescriptor};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationAuthorizationDescriptor {
    pub actions: Vec<NotificationAuthorizationAction>,
    pub managed_secret_keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationAuthorizationAction {
    pub code: String,
    pub label: String,
    pub kind: NotificationAuthorizationActionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationAuthorizationActionKind {
    QrCode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum NotificationAuthorizationOperation {
    Begin { action: String },
    Poll,
    Cancel,
    Maintain,
}

impl<'de> Deserialize<'de> for NotificationAuthorizationOperation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Internally tagged unit variants otherwise ignore extra fields, including credentials.
        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
        enum StrictOperation {
            Begin { action: String },
            Poll {},
            Cancel {},
            Maintain {},
        }

        Ok(match StrictOperation::deserialize(deserializer)? {
            StrictOperation::Begin { action } => Self::Begin { action },
            StrictOperation::Poll {} => Self::Poll,
            StrictOperation::Cancel {} => Self::Cancel,
            StrictOperation::Maintain {} => Self::Maintain,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationAuthorizationStatus {
    Unauthorized,
    Pending,
    Authorized,
    Invalid,
}

/// Canonical client DTO. Enabled/disabled is an independent administrator setting.
/// No credential, recipient id, context token or provider error belongs in this view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationAuthorizationView {
    pub status: NotificationAuthorizationStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_action: Option<NotificationAuthorizationUserAction>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationAuthorizationUserAction {
    pub kind: NotificationAuthorizationActionKind,
    /// Displayable QR URL only. Internal polling credentials belong in managed_secret_patch.
    pub qr_code_url: String,
    pub poll_interval_seconds: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

/// Host-injected secrets are separate from user config and never client supplied.
/// Includes encrypted QR credentials, bot token, bound scanned user id and inbound cursor/context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationRuntimeInput {
    pub operation: NotificationOperation,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub managed_secrets: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum NotificationOperation {
    Authorize {
        operation: NotificationAuthorizationOperation,
    },
    Send {
        input_payload: Value,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationFailureCode {
    AuthorizationRequired,
    AuthorizationInvalid,
    RecipientNotReady,
    ProviderRejected,
    ProviderResultUnknown,
}

/// Unknown is a separate terminal delivery state, never a confirmed failure or success.
/// Only a provider-confirmed success may return a schema-checked result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum NotificationSendOutcome {
    Succeeded {
        result: Value,
    },
    Failed {
        failure_code: NotificationFailureCode,
    },
    Unknown {
        failure_code: NotificationFailureCode,
    },
}

impl NotificationSendOutcome {
    pub fn unknown() -> Self {
        Self::Unknown {
            failure_code: NotificationFailureCode::ProviderResultUnknown,
        }
    }

    pub fn validate(&self, descriptor: &NotificationTypeDescriptor) -> ContractResult {
        match self {
            Self::Succeeded { result } => descriptor.validate_output(result),
            Self::Failed {
                failure_code: NotificationFailureCode::ProviderResultUnknown,
            } => Err(invalid("ambiguous send outcome must be unknown")),
            Self::Unknown { failure_code }
                if *failure_code != NotificationFailureCode::ProviderResultUnknown =>
            {
                Err(invalid("unknown outcome requires provider_result_unknown"))
            }
            _ => Ok(()),
        }
    }
}

/// Host-only result envelope. Persist the patch encrypted before returning only the public view.
/// Null patch values delete keys; omitted keys stay unchanged. Never log this envelope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum NotificationRuntimeResult {
    Authorization {
        authorization: NotificationAuthorizationView,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        managed_secret_patch: BTreeMap<String, Value>,
    },
    Send {
        outcome: NotificationSendOutcome,
        /// Omitted means no change; recipient_not_ready must not revoke account authorization.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        authorization: Option<NotificationAuthorizationView>,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        managed_secret_patch: BTreeMap<String, Value>,
    },
}

impl NotificationAuthorizationDescriptor {
    pub fn validate(&self) -> ContractResult {
        let mut actions = BTreeSet::new();
        let mut keys = BTreeSet::new();
        if self.actions.is_empty()
            || self.actions.iter().any(|action| {
                !identity(&action.code)
                    || action.code.contains('/')
                    || action.label.trim().is_empty()
                    || !actions.insert(&action.code)
            })
        {
            return Err(invalid("authorization requires unique labeled actions"));
        }
        if self.managed_secret_keys.is_empty()
            || self
                .managed_secret_keys
                .iter()
                .any(|key| !identity(key) || key.contains('/') || !keys.insert(key))
        {
            return Err(invalid("authorization requires unique managed secret keys"));
        }
        Ok(())
    }

    pub fn validate_secret_keys(&self, secrets: &BTreeMap<String, Value>) -> ContractResult {
        if secrets
            .keys()
            .any(|key| !self.managed_secret_keys.contains(key))
        {
            return Err(invalid("undeclared managed secret key"));
        }
        Ok(())
    }
}

impl NotificationAuthorizationView {
    pub fn validate(&self) -> ContractResult {
        if (self.status == NotificationAuthorizationStatus::Pending) != self.user_action.is_some() {
            return Err(invalid("only pending authorization requires a user action"));
        }
        if let Some(action) = &self.user_action {
            let url = url::Url::parse(&action.qr_code_url)
                .map_err(|_| invalid("QR action requires a displayable URL"))?;
            if url.scheme() != "https"
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || action.poll_interval_seconds == 0
            {
                return Err(invalid(
                    "QR action requires a displayable URL and positive polling interval",
                ));
            }
        }
        Ok(())
    }
}

impl NotificationRuntimeInput {
    pub fn validate(&self, descriptor: &NotificationTypeDescriptor) -> ContractResult {
        let authorization = descriptor
            .authorization
            .as_ref()
            .ok_or_else(|| invalid("notification has not opted into authorization"))?;
        authorization.validate_secret_keys(&self.managed_secrets)?;
        match &self.operation {
            NotificationOperation::Authorize {
                operation: NotificationAuthorizationOperation::Begin { action },
            } if !authorization
                .actions
                .iter()
                .any(|declared| &declared.code == action) =>
            {
                Err(invalid("undeclared notification authorization action"))
            }
            NotificationOperation::Send { input_payload } => {
                descriptor.validate_input(input_payload)
            }
            _ => Ok(()),
        }
    }
}

impl NotificationRuntimeResult {
    pub fn validate_for(
        &self,
        input: &NotificationRuntimeInput,
        descriptor: &NotificationTypeDescriptor,
    ) -> ContractResult {
        input.validate(descriptor)?;
        let declared = descriptor
            .authorization
            .as_ref()
            .expect("validated opt-in descriptor");
        match (self, &input.operation) {
            (
                Self::Authorization {
                    authorization,
                    managed_secret_patch,
                },
                NotificationOperation::Authorize { .. },
            ) => {
                authorization.validate()?;
                declared.validate_secret_keys(managed_secret_patch)
            }
            (
                Self::Send {
                    outcome,
                    authorization,
                    managed_secret_patch,
                },
                NotificationOperation::Send { .. },
            ) => {
                outcome.validate(descriptor)?;
                if let Some(view) = authorization {
                    view.validate()?;
                    let expected = match outcome {
                        NotificationSendOutcome::Succeeded { .. } => {
                            Some(NotificationAuthorizationStatus::Authorized)
                        }
                        NotificationSendOutcome::Failed {
                            failure_code: NotificationFailureCode::AuthorizationRequired,
                        } => Some(NotificationAuthorizationStatus::Unauthorized),
                        NotificationSendOutcome::Failed {
                            failure_code: NotificationFailureCode::AuthorizationInvalid,
                        } => Some(NotificationAuthorizationStatus::Invalid),
                        NotificationSendOutcome::Failed {
                            failure_code: NotificationFailureCode::RecipientNotReady,
                        } => Some(NotificationAuthorizationStatus::Authorized),
                        _ => None,
                    };
                    if expected.is_some_and(|status| view.status != status) {
                        return Err(invalid("send outcome conflicts with authorization status"));
                    }
                } else if matches!(
                    outcome,
                    NotificationSendOutcome::Failed {
                        failure_code: NotificationFailureCode::AuthorizationRequired
                            | NotificationFailureCode::AuthorizationInvalid
                    }
                ) {
                    return Err(invalid(
                        "authorization failure requires an updated public status",
                    ));
                }
                declared.validate_secret_keys(managed_secret_patch)
            }
            _ => Err(invalid(
                "notification response does not match requested operation",
            )),
        }
    }
}

# Notification instances and plugin sends

Notification types, workspace instances and recipient inbox messages have separate owners. Settings stores instance configuration; a workflow notification node selects an instance and supplies invocation input. The Host provides only the built-in `system` type. Managed plugins contribute other types.

## Persistence and delivery

`notification_instances` stores workspace identity, type, installation, display name, status, public configuration and encrypted secrets. Password controls identify secret string fields. Read responses omit secret values and return only `configured_secret_fields`; updates retain omitted secrets.

`system_notifications` stores one message and independent `read_at` per recipient. Every recipient must be an active member of the current workspace. The transaction commits all recipients before publishing a refresh signal. Inbox reads, unread counts and read operations filter by workspace and authenticated recipient. Deleting an instance or disabling/uninstalling a plugin preserves messages. Sending through a missing, disabled or unavailable instance fails explicitly.

A system send succeeds when the message transaction commits. An external result represents the provider's declared receipt confirmation, not a user read. The credentialed `notification.changed` SSE carries refresh signals only. Opening or reconnecting the stream refetches the HTTP/database snapshot; the stream is not a durable message queue.

## Authoring a type

The [complete fixture manifest](../../api/plugins/fixtures/acme.notification/manifest.yaml) declares `notification_types`. Each descriptor has a namespaced `type_code` (`publisher_namespace/plugin_id/local_type`), `display_name`, `description`, `contribution_id`, separate `config_schema`/`config_form`, `input_schema`/`input_form`, and an object `output_schema`. The Host reserves `system`.

Forms use the existing `PluginFormSchema` version `1.0.0` and string, number, boolean or select controls, defaults and conditions. `password` marks configuration string fields; `multi-select` represents an array and validates each option against `items`. Schemas are self-contained and cannot resolve external references. The built-in system input form receives current-workspace member options from the types API.

Bind a contribution to `1flowbase.notification.send`, version `1flowbase.notification/v1`, requiring `notification.send`, with binding payload `{kind: notification_type, type_code: ...}`. Installation, workspace assignment, independent contribution grants and activation follow the existing managed lifecycle. A declaration does not grant execution. Grant `permission_contract_id: notification`, `permission_contract_version: 1`, with workspace resource scope.

## Worker execution

The existing capability envelope carries `method: execute` with `input.config_payload` and `input.input_payload`. The shared Host owns instance policy, trusted actor/workspace, frozen contribution admission, authority checks, process dispatch and lifecycle. The plugin owns provider delivery and its declared result. It needs no duplicate Host transport or database connection.

Rust workers can call `runtime_extension_sdk::execute_notification`. The [SDK worker](../../api/crates/runtime-extension-sdk/src/_tests/notification_worker.rs) demonstrates one-shot stdio. A multiplex worker uses the existing `serve` handler, deserializes `CapabilityStdioRequest`, invokes the same helper and serializes the response. This release supports process-per-call stdio and multiplex; it does not promise an actor service pool for notification types.

Build `cargo build --locked --manifest-path api/Cargo.toml -p runtime-extension-sdk --example notification_worker`, resolve the executable from the configured Cargo target directory, then package `api/plugins/fixtures/acme.notification` through `scripts/node/plugin/cli.js package --runtime-binary <absolute executable>`. The fixture does not contact an external provider. No SSH/OpenAI plugin changes or publication are required.

## Workflow and HTTP

The native node has `type: notification` and `config.instance_id`. `bindings.input` uses existing `named_bindings` entries with descriptor field names, constant/selector/templated_text expressions and `valueType: number` for numeric expressions. The runtime forwards the resolved input object to the shared service. Output is `{instance_id, type_code, result}`; the default result variable uses the selected type's output schema and supports nested downstream selectors.

Published and resumed runs retain their frozen actor/workspace even if the user's current workspace later changes. A single-node preview sends once without replaying upstream nodes. The same service handles workflow and HTTP sends.

`/api/console/notifications` owns type discovery, instance CRUD/send, recipient messages, unread counts, read/read-all, and refresh events. Settings methods have independent console operations; authenticated types and recipient inbox APIs do not require member-management permission. Business HTTP enters the Canonical Interface and typed Handler. Events is a read-only protocol control entry.

Real package intake, PostgreSQL, independent grants and SDK binary fixtures cover sends, schema failures, secret redaction and retained unavailable instances. Actual acceptance evidence is recorded under the task's `tmp/test-governance/`; examples and source fixtures alone do not constitute a test pass.

## Send logs and personal channels

Every authorized workspace send persists one pending `notification_send_logs` fact before validation or execution. The shared `NotificationSenderPort` receives a trusted `api` or `workflow` source from its caller; HTTP bodies cannot select it. Instance and actor UUIDs have no foreign keys and historical sends are not backfilled. Missing instance names/types are nullable. Log list/get have independent workspace-scoped settings operations.

System messages and log success commit in one transaction, recording only safe system title/content and deduplicated recipient count. Plugin config, input, result and raw errors are never logged. Pre-send instance/type/plugin/input/config/recipient rejection has a fixed failure code. Provider errors may follow an external effect and become unknown/provider_result_unknown. Unsafe or schema-invalid provider output becomes unknown/provider_output_invalid. Failed finalization leaves pending rather than asserting definite failure.

The channels endpoint aggregates only the current workspace and authenticated recipient. It applies all/unread/read before choosing each channel's latest message, orders by that latest time, and independently counts all unread messages per channel. Cursor pages are bounded to 100. Messages support instance_id/read_status; explicit read_status takes precedence over the retained unread_only parameter. LEFT JOIN preserves deleted-instance history with deleted=true, display_name='Deleted notification channel', type_code='system', because the inbox table contains only system messages.

Message unread clears only the recipient's read_at. Channel read-all changes only that recipient/channel, including deleted-channel history. Both publish recipient/workspace refresh signals. Existing global read-all remains available.

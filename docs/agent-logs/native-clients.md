# Native client collection

[简体中文](native-clients.zh-CN.md)

Native hooks and plugin callbacks invoke these collectors. The shared SDK owns private configuration, durable pending delivery, stable event IDs, incremental checkpoints and ACK validation. Installation does not scan or upload all history, and no separate resident watcher is required.

| Client | Integration | Version |
| --- | --- | --- |
| Codex | Native hooks targeting the current transcript | 0.4.0 |
| Claude Code | Settings hooks targeting the current transcript | 0.1.0 |
| DeepSeek Harness | Cordis session events and turn/checkpoint callbacks | 0.1.0 |
| OpenCode | Default V1 message/part events, plus V2 session events | 0.1.0 |

DSH/OpenCode callbacks synchronously persist native records before returning. Idle/checkpoint/turn-end notifications trigger delivery; idle alone never proves model success. Failed delivery remains queued for a later callback or explicit flush.

## Configuration and recovery

Use the application-specific Agent Logs installer after its signed release is published. A locally built executable also supports:

~~~sh
COLLECTOR configure --endpoint https://HOST/api/logs/v1/events --config /PRIVATE/config.json --source /PRIVATE/source --key-stdin
COLLECTOR install-hooks --config /PRIVATE/config.json --client-home /CLIENT/HOME
COLLECTOR flush --config /PRIVATE/config.json
COLLECTOR uninstall-hooks --config /PRIVATE/config.json --client-home /CLIENT/HOME
~~~

Replace COLLECTOR with the selected CLIENT-logs-collector. Codex retains --codex-home. Claude sources point to its transcript home; DSH/OpenCode sources are private collector journals. Send the log application's key through stdin. The model-inference gateway key belongs in the client's gateway configuration and serves a different purpose.

Registration preserves other hooks/configuration. Uninstall removes only owned registration, retaining configuration and checkpoints. Import is explicit and never invoked by installation. Resuming a Claude session can cause its hook to read that current session's existing transcript.

Canonical input_tokens includes ordinary input, cache reads and cache writes. Unknown values remain unknown; omitted counters become zero only when the native contract defines that meaning. Completion, failure and cancellation require native terminal facts. A final answer requires successful completion and actual text.

## Validation configuration

The application publication must declare the requested model. Check the selected key's /v1/models first.

- Claude Code: Anthropic service-root base URL, gpt-6-luna, --effort max.
- DSH: Chat/Responses base URLs end in /v1; Anthropic uses the service root. Set model reasoningEfforts: {max: max} and compat: {forceAdaptiveThinking: true} to send adaptive thinking and output_config.effort=max.
- Default OpenCode V1: provider.npm is @ai-sdk/openai-compatible, @ai-sdk/openai or @ai-sdk/anthropic respectively. Configure baseURL/apiKey in options and use object-valued variants. Chat/Responses use reasoningEffort: max; Anthropic uses effort: max with thinking: {type: adaptive}. Select --variant max.

Verify actual requests, tool results, continuation and stored logs. Exit codes or configured names alone do not prove protocol or effort. Background title requests are separate from agent turns; isolated tests can disable OpenCode's title agent.

See each collector's bilingual README in the official plugin repository for source versions, event semantics and installation limitations.

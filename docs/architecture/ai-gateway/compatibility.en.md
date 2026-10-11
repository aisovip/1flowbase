# Responses compatibility across protocols

The gateway separates protocol mapping, AI Native semantics, and provider adaptation. Compatibility preserves representable semantics; it does not imply that every provider supports every Responses feature.

## Notifications and tool results

A `function_call_output` with an absent or null `call_id` is standalone external context. The mapping layer preserves its JSON output, name, namespace, and extensions. AI Native stores typed external context and uses a shared projection into an attributed user-level data message. It neither elevates the content to system/developer instructions nor fabricates a tool call ID. Attribution and content remain external data, not trusted model instructions.

Only nonempty call IDs participate in tool-result correlation. Empty or malformed IDs are rejected. Both full-history and `previous_response_id` continuations preserve notification/result order and continue to verify scope, complete result sets, duplicates, and history proofs. A notification cannot complete or reopen a tool task.

## Transport selection and response style

Requests containing notifications or only `text.verbosity` retain both a semantic representation and the original Responses body. A selected provider supporting both `responses.native_passthrough` and `responses.native_output.v1` receives the original body; other providers use the semantic representation. Required native fields and existing provider continuation affinity cannot fall back. Recovery, output, and history metadata follow the transport actually selected.

On the semantic path, `text.verbosity` values low, medium, and high become a request-owned response style prompt, recorded as Normalized in the translation report. Model adherence is best effort and is not equivalent to a native provider parameter. Unknown text extensions and `text.format` still require native capabilities; invalid verbosity values are rejected.

## Provider responsibilities

The DeepSeek plugin lowers Native tool choices into Chat Completions: auto, none, and required become strings; named choices become function objects. The host owns notification projection, subprocess communication, and continuation state. Plugins do not need their own Responses emulation.

This path supports ordinary text, standard function tools, and the notifications described above. It is not complete Codex compatibility: custom grammar tools, strict tool schemas, hosted capabilities, and provider-private reasoning remain subject to their capability checks. Silently dropping fields is not a substitute for support.

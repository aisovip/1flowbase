# Agent Logs exchange runtime scenario

Run this scenario **after** the Root's centralized test batch and candidate runtime are ready. It installs no packages, starts no services, and changes no existing application. It creates three clearly named synthetic demo applications, each with its own temporary API key. The applications and their imported logs remain available for review; their scenario keys are revoked in `finally`.

Prerequisites:

- Explicit candidate API and frontend origins, including the new exchange routes.
- Main checkout root credentials readable by `page-debug/auth.js`; no direct sign-in implementation is present here.
- A private Langfuse JSON file with `{base_url,public_key,secret_key,project_id,email,password}` and a running Langfuse worker. Email/password are validated as supplied private context, not used for an ad hoc login.
- A separately prepared `node_modules` containing `openlit@1.15.0` and its `@opentelemetry/api` dependency. No project dependency changes are needed.
- Playwright from the main checkout's `web/` installation and `/usr/bin/google-chrome`.

From the candidate checkout:

```bash
node scripts/node/agent-logs-exchange/scenario.cjs \
  --api-base-url http://127.0.0.1:7801 \
  --web-base-url http://127.0.0.1:3102 \
  --credentials-root /home/taichuy/git/1flowbase \
  --langfuse-private-file /home/taichuy/git/1flowbase/tmp/test-governance/agent-logs-otlp/langfuse-private.json \
  --openlit-module-root /home/taichuy/git/1flowbase/tmp/test-governance/agent-logs-otlp/openlit-runtime/node_modules \
  --gateway-key-file /home/taichuy/git/1flowbase/docs/draft/tmp-key/1flowbase-7800-key-codex.md \
  --langfuse-storage-private-file /home/taichuy/git/1flowbase/tmp/test-governance/agent-logs-otlp/langfuse-storage-private.json \
  --out-dir tmp/test-governance/agent-logs-otlp/runtime-exchange \
  --deadline-ms 120000
```

The gateway-key flag is optional. When present it must contain exactly one `sk-` token on a single line. The scenario makes exactly two short requests to the existing gateway at `http://127.0.0.1:7800/v1/chat/completions`, with `gpt6-luna` and `reasoning_effort: max`; returned answer text and actual token usage become SDK span attributes. It does not request reasoning text or change gateway configuration. Without this flag, the two SDK spans carry explicitly synthetic messages and token counts. `message_source` in the receipt distinguishes the two modes. Both modes use the actual OpenLIT SDK exporter.

The browser fixture selects the two visible canonical rows from the three imported tasks, executes real Langfuse export on desktop and mobile, and asserts all three records, 21 spans and the API-verified two Trace IDs. It then repeats the Langfuse import through the UI and checks zero accepted events, 21 duplicates and all three retained record IDs. Business locators determine readiness; notification streams do not need to become idle.

The required Langfuse storage-state flag captures its traces page using the supplied existing private session. It is required to produce all five Wiki images. It does not claim a screenshot proves every exported observation; the v2 API readiness check supplies that evidence. Root may capture additional trace detail screens manually.

## Assertions and artifacts

The canonical upload contains 21 explicitly synthetic events across three tasks and two sessions, covering context, user, tool call/result, final assistant answer, usage and task completion. Each source session corresponds to one Trace; tasks retain their source anchor hierarchy in the Span tree. These three task records therefore yield two Trace IDs and 21 spans, rather than one Trace per record. The scenario:

1. Uploads these source facts through `/api/logs/v1/events` with the new source application's key.
2. Exports all three record IDs to an actual OTLP JSON file, reads that file back, imports into a separate demo app and verifies a second import returns exactly 21 duplicates and zero accepted events.
3. Exports the same three records to real Langfuse, checks three exported records and all 21 event spans, asserts the distinct Trace ID count matches the fixture source sessions and the returned Trace ID set exactly matches the downloaded OTLP file, and waits until `/api/public/v2/observations` contains all exported spans for all returned trace IDs. Readiness follows the actual local v2 response `meta.cursor` with request `cursor`; it also recognizes a `meta.nextCursor` variant and rejects conflicting nonempty values. It checks every cursor advances, deduplicates observation IDs across pages, never stops at the first page while a cursor remains, and and deliberately requests two observations per transport page (`fields=basic`) to exercise pagination. This fixture page size does not limit exported data. A configurable deadline and all persisted observations determine readiness, not a fixed sleep. A third application imports the trace IDs, then repeats the import to assert duplicates.
4. Reads record overview and record client-trajectory routes. It checks source identity, session/task ownership, user/final messages, token usage, absence of fabricated native runs, and **each original event's full raw section** including event ID, context, tool facts and usage.
5. Creates two recording `@opentelemetry/api` spans through OpenLIT 1.15.0, calls `forceFlush()` and shuts down its owned provider. SDK configuration is `otlpEndpoint: candidate + '/api/logs/otlp'`; this SDK appends `/v1/traces` itself. The scenario polls candidate record APIs and asserts model/usage/session attribution reached storage, then reads paginated client-trajectory raw sections. Generic `gen_ai.input.messages` is LLM history Context, and ordinary output is Assistant without a source final declaration; the scenario verifies prompt/answer and original span attributes are retained while no TaskEnd/final phase is fabricated. It does not require these SDK facts to appear as human/final overview messages. SDK diagnostics are saved only to `private/openlit-sdk.log` (mode 0600), never printed; a successful SDK child exit alone is insufficient.
6. Uses a separate temporary console session at desktop 1440×1000 and mobile 390×844. It captures all five Wiki image categories: selected source records before opening a modal; masked Langfuse export configuration; the Langfuse traces page; masked import configuration in the actual target app with the returned Trace IDs; and an imported conversation containing the original source user message and declared final answer. On both viewports it executes a real repeat Langfuse import through the UI, asserts the HTTP receipt has zero accepted events / 21 duplicates and unchanged record IDs, then checks those counts are rendered. Credentials are masked in every configuration/receipt screenshot. Browser and console session are disposed in `finally`; private temporary storage state is removed.

The desktop Wiki files are `01-record-selection.png`, `02-langfuse-export.png`, `03-langfuse-traces.png`, `04-langfuse-import.png`, and `05-imported-conversation.png`; the matching mobile files have a `mobile-` prefix. Additional OTLP modal and actual import-receipt screenshots are retained for runtime evidence. Screenshot assertions use the canonical fixture's expected user/final text, selected by the verified imported record's source task identity.

`runtime-receipt.json` records the source SHA, actual completed checks, created application IDs, trace IDs, screenshot paths, safe failure stages, and cleanup. It includes no credentials or application API key token. `synthetic-otlp.json` contains only this scenario's synthetic data; `openlit-summary.json` contains SDK span IDs and session IDs. Runtime artifacts stay under candidate `tmp/test-governance/`. SDK diagnostics remain private; browser state is transient under a mode-0700 directory, never committed.

Exit code is nonzero for any assertion, HTTP, browser, SDK or session/key cleanup failure. A failure stage or safe HTTP status is recorded without serializing API bodies or exception diagnostics. Partial checks remain visible in the receipt and do not become a successful run. Demo applications can be removed manually after Root review.

The CLI and fixtures have only been syntax-checked during development. No runtime, gateway request, browser capture or service launch is performed by writing this scenario.

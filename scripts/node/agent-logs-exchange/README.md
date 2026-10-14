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
  --gateway-key-file /home/taichuy/git/1flowbase/docs/draft/tmp-key/1flowbase.md \
  --langfuse-storage-private-file /home/taichuy/git/1flowbase/tmp/test-governance/agent-logs-otlp/langfuse-storage-private.json \
  --out-dir tmp/test-governance/agent-logs-otlp/runtime-exchange \
  --deadline-ms 120000
```

The gateway-key flag is optional. When present it must contain exactly one `sk-` token on a single line. The scenario makes exactly two short requests to the existing gateway at `http://127.0.0.1:7800/v1/chat/completions`, with `gpt-6-luna` and `reasoning_effort: max`; returned answer text and actual token usage become SDK span attributes. It does not request reasoning text or change gateway configuration. Without this flag, the two SDK spans carry explicitly synthetic messages and token counts. `message_source` in the receipt distinguishes the two modes. Both modes use the actual OpenLIT SDK exporter.

The optional Langfuse storage-state flag captures its traces page using the supplied existing private session. It does not claim a screenshot proves every exported observation; the v2 API readiness check supplies that evidence. Root may capture additional trace detail screens manually.

## Assertions and artifacts

The canonical upload contains 21 explicitly synthetic events across three tasks and two sessions, covering context, user, tool call/result, final assistant answer, usage and task completion. The scenario:

1. Uploads these source facts through `/api/logs/v1/events` with the new source application's key.
2. Exports all three record IDs to an actual OTLP JSON file, reads that file back, imports into a separate demo app and verifies a second import returns exactly 21 duplicates and zero accepted events.
3. Exports the same three records to real Langfuse, checks the export count, and waits until `/api/public/v2/observations` contains all exported spans for all returned trace IDs. Readiness uses a configurable deadline and actual persisted observations, not a fixed sleep. A third application imports the trace IDs, then repeats the import to assert duplicates.
4. Reads record overview and record client-trajectory routes. It checks source identity, session/task ownership, user/final messages, token usage, absence of fabricated native runs, and **each original event's full raw section** including event ID, context, tool facts and usage.
5. Creates two recording `@opentelemetry/api` spans through OpenLIT 1.15.0, calls `forceFlush()` and shuts down its owned provider. SDK configuration is `otlpEndpoint: candidate + '/api/logs/otlp'`; this SDK appends `/v1/traces` itself. The scenario polls candidate record APIs and asserts real SDK messages/model/usage/session attribution reached storage. SDK diagnostics are saved only to `private/openlit-sdk.log` (mode 0600), never printed; a successful SDK child exit alone is insufficient.
6. Uses a separate temporary console session to open the new source app at desktop 1440×1000 and mobile 390×844, check two existing record checkboxes, and capture OTLP/Langfuse exchange modals. Both Langfuse key inputs are masked. Browser and console session are disposed in `finally`; private temporary storage state is removed.

`runtime-receipt.json` records the source SHA, actual completed checks, created application IDs, trace IDs, screenshot paths, safe failure stages, and cleanup. It includes no credentials or application API key token. `synthetic-otlp.json` contains only this scenario's synthetic data; `openlit-summary.json` contains SDK span IDs and session IDs. Runtime artifacts stay under candidate `tmp/test-governance/`. SDK diagnostics remain private; browser state is transient under a mode-0700 directory, never committed.

Exit code is nonzero for any assertion, HTTP, browser, SDK or session/key cleanup failure. A failure stage or safe HTTP status is recorded without serializing API bodies or exception diagnostics. Partial checks remain visible in the receipt and do not become a successful run. Demo applications can be removed manually after Root review.

The CLI and fixtures have only been syntax-checked during development. No runtime, gateway request, browser capture or service launch is performed by writing this scenario.

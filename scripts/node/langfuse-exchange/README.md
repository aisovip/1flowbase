# Langfuse managed-plugin acceptance

This finite scenario targets an independently running exact-source host and installed
public `@taichuy/langfuse` plugin. Build the host/static web/plugin with the GitHub
Actions evidence workflows; do not run local Cargo or cold frontend builds.

Use `install.cjs`'s `install` function with an explicit package, parsed public manifest,
`apiBaseUrl`, `credentialsRoot`, and `outDir`. It exercises the existing package
intake, explicit contribution grants and system activation APIs. It permits only
known unsigned development-package risks and saves no credentials.

Then run:

```sh
node scripts/node/langfuse-exchange/scenario.cjs \
  --api-base-url http://127.0.0.1:7801 \
  --web-base-url http://127.0.0.1:3102 \
  --credentials-root /absolute/path/to/1flowbase \
  --langfuse-private-file /absolute/path/to/private-project-credentials.json \
  --langfuse-storage-private-file /absolute/path/to/private-langfuse-storage.json \
  --out-dir /absolute/path/to/candidate/tmp/test-governance/langfuse-plugin/browser
```

The private project file provides `base_url`, `public_key`, `secret_key`, and
`project_id`; browser storage is an existing authenticated Langfuse session. Never
commit either file. Authentication uses the host's temporary session owner and is
revoked in `finally`.

The scenario creates two labeled Agent Logs applications, ingests a controlled
21-event / 3-task / 2-conversation source, and reads retained Agent Flow history
without model calls. `--native-application-id` optionally chooses that history.
On desktop and mobile it saves/reloads the connection, checks the real remote,
exports both sources by date, waits for remote persistence, imports into the chosen
Agent Logs application, repeats import, compares exact original source facts, and
captures actual pages. The source application keys are revoked on exit; labeled
applications and imported history remain available for manual inspection.

Receipts and screenshots go beneath `tmp/test-governance/`. A resource supervisor
must stop only this task's runtime/browser processes if system CPU, physical memory,
or disk occupancy reaches 90%. Ports 7600 and 7800 are protected and rejected.

Existing standard OTLP codec, authorization, native archive and source-reader
regressions run in `agent-logs-otlp.yml`; the plugin's own SDK callback and mapping
fixtures run in the official repository's `langfuse-ci.yml`.

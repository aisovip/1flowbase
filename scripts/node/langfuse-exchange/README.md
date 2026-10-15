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
captures actual pages. It also imports the current synthetic traces by date and checks source/target/date label associations and unique form IDs. Source selection follows the managed component prefix and real virtual-list scrolling; no hidden UI state is set. The source application keys are revoked independently on exit (including normal DELETE204 responses); labeled
applications and imported history remain available for manual inspection.

Receipts and screenshots go beneath `tmp/test-governance/`. A resource supervisor
must stop only this task's runtime/browser processes if system CPU, physical memory,
or disk occupancy reaches 90%. Ports 7600 and 7800 are protected and rejected.

Existing standard OTLP codec, authorization, native archive and source-reader
regressions run in `agent-logs-otlp.yml`; the plugin's own SDK callback and mapping
fixtures run in the official repository's `langfuse-ci.yml`.

Remote persistence is verified by complete, trace-bound observations v2 cursor reads,
with a narrow older-deployment capability fallback to the numbered observations API.
Duplicate remote bookkeeping versions are compared by semantic facts; foreign traces,
real conflicts and pagination loops fail acceptance. HTTP200 or an enqueue receipt alone
does not pass the persistence check. External screenshots open the exact current synthetic
Trace detail and timestamp, not a project-wide history table. Keys are masked, and final
publication still requires visual inspection. All original21-fact/raw/dedup assertions remain.

The helper regression tests are `node --test scripts/node/langfuse-exchange/_tests/*.test.cjs`.

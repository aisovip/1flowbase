# Exchange Agent Logs with OTLP and Langfuse

[中文](otlp-exchange.zh-CN.md)

> Local documentation draft: screenshots and live interoperability checks are pending centralized acceptance. The image URLs below are reserved for real captured pages; they are not evidence of a completed exchange.

Use OTLP JSON to move selected log records between compatible systems, send several records to a Langfuse project, or import several Langfuse traces into a chosen 1flowbase application. Existing native collectors keep their installation, event upload and recovery behavior.

## Prepare the applications and credentials

Create an **Agent Logs** application and an application API key if you will send SDK traces. Enable **Log reception**. For console exchanges, sign in to the correct workspace and obtain the operation permission for the exchange you need. An application key authorizes trace ingestion; it does not authorize console exchange operations.

For Langfuse, prepare your project's **Public Key**, **Secret Key**, and deployment base URL, such as `https://YOUR_LANGFUSE_HOST`. Use the deployment URL, not a trace page or `/api/public/otel/v1/traces`. Your 1flowbase server must be able to reach that deployment. The browser dialog keeps credentials in memory for the current exchange and clears them when it closes; Secret Key is masked.

Logs can contain prompts, code and tool outputs. Select the records and destination you intend to share.

## Select records and export OTLP JSON

1. Open the application's **Logs** page and select several rows.
2. Open **Exchange logs**, choose **Export OTLP JSON**, then **Execute exchange**.
3. Save the downloaded JSON file. It is a standard OTLP trace payload, not a platform backup or a native collector archive.

On the global logs page, selected records are grouped by application and exported separately. There is no implicit cross-application target for an import: select the target application explicitly.

![Select several log records and open Exchange logs](https://raw.githubusercontent.com/wiki/taichuy/1flowbase/assets/agent-logs-otlp/01-record-selection.png)

To import a file, open **Exchange logs**, choose **Import OTLP JSON**, choose an OTLP JSON object file, and execute the exchange. Review the server receipt: `accepted_events` and `duplicate_events` count events; `record_ids` identifies the resulting log records. Event counts are not conversation counts.

## Export several records to Langfuse

1. Select the records, open **Exchange logs**, and choose **Export to Langfuse**.
2. Fill in **Langfuse URL**, **Public Key** and **Secret Key** for the destination project.
3. Execute the exchange and review `exported_records`, `exported_spans` and `trace_ids`. If the receipt reports fewer records than selected, investigate that partial result before assuming every record arrived.
4. Open the same project in Langfuse and locate the returned trace IDs. Inspect input, output, spans and metadata against the selected source records.

![Configure a multi-record export to Langfuse](https://raw.githubusercontent.com/wiki/taichuy/1flowbase/assets/agent-logs-otlp/02-langfuse-export.png)

![Locate exported traces in the destination Langfuse project](https://raw.githubusercontent.com/wiki/taichuy/1flowbase/assets/agent-logs-otlp/03-langfuse-traces.png)

The server sends OTLP traces to Langfuse's `/api/public/otel/v1/traces` with HTTP Basic authentication using the Public Key and Secret Key. The credentials belong to Langfuse; they are not 1flowbase application keys.

Trace identity follows the source/session pair. Tasks and events become the span hierarchy within that Trace, so several task records in one session can share a Trace ID. Compare `exported_records` with the selected record count, rather than with the length of `trace_ids`. For example, a fixture containing three tasks across two sessions maps to two traces and 21 spans; that is a mapping example, not a claim of completed live interoperability verification.

## Import several traces from Langfuse

1. Open the target application's **Logs** page, or explicitly select the target on the global logs page.
2. Choose **Import from Langfuse** in **Exchange logs** and enter the deployment URL and project keys.
3. Paste the desired **Trace IDs**, separated by commas or newlines, and execute the exchange.
4. Review `accepted_events`, `duplicate_events` and `record_ids`. Open those records to inspect the imported conversation and trajectory. Repeated identities with unchanged payloads are duplicates; a conflicting payload under the same identity is an error.

![Choose a target and enter multiple Langfuse trace IDs](https://raw.githubusercontent.com/wiki/taichuy/1flowbase/assets/agent-logs-otlp/04-langfuse-import.png)

![Inspect an imported record and its conversation](https://raw.githubusercontent.com/wiki/taichuy/1flowbase/assets/agent-logs-otlp/05-imported-conversation.png)

Remote import reads observation data for the requested Trace IDs from Langfuse's public observations-v2 API, including all observation pages. It can use the legacy observations API only after an explicit capability-not-found (404) response. It does not require a separate traces API to supply the data. Authentication, network and server failures should be resolved rather than interpreted as a legacy API signal.

## Send OpenLIT SDK traces directly

The receiver is **HTTP Trace only**: JSON or protobuf, optionally gzip, at `POST /api/logs/otlp/v1/traces`. This integration does not provide gRPC, Metrics or Logs receivers. Use an application API key as `Authorization: Bearer <APPLICATION_KEY>`.

The following configurations match OpenLIT TypeScript **1.15.0** and Python **1.45.0**. Initialize instrumentation before the SDK calls you want to observe, using the initialization/import order required by your framework. OpenLIT produces the facts available from the instrumented SDK; it does not automatically capture every human message, agent tool, or task completion.

### TypeScript

```typescript
import OpenLIT from 'openlit';

OpenLIT.init({
  otlpEndpoint: 'https://YOUR_HOST/api/logs/otlp',
  otlpHeaders: { Authorization: 'Bearer <APPLICATION_KEY>' },
  disableMetrics: true,
  disableEvents: true,
});
```

This SDK uses the HTTP JSON trace exporter and appends `/v1/traces` to `otlpEndpoint`. Do not put that suffix in this base URL twice.

### Python

```python
import os

# Set before importing OpenLIT so the HTTP exporter is selected.
os.environ['OTEL_EXPORTER_OTLP_PROTOCOL'] = 'http/protobuf'
os.environ['OTEL_EXPORTER_OTLP_TRACES_ENDPOINT'] = (
    'https://YOUR_HOST/api/logs/otlp/v1/traces'
)

import openlit

openlit.init(
    otlp_headers={'Authorization': 'Bearer <APPLICATION_KEY>'},
    disable_metrics=True,
    disable_events=True,
)
```

Python uses the HTTP protobuf exporter by default unless gRPC is configured. The signal-specific trace endpoint above is the full URL. After a real instrumented call, allow the SDK's batch exporter to flush before process exit, then check the application's log records and trajectory.

OpenLIT's CLI can emit Metrics alongside Trace. Configure each additional signal for a backend that supports it; pointing the CLI's general endpoint at this Trace receiver does not establish support for all CLI telemetry.

## HTTP contract

All four console endpoints below use `POST /api/console/applications/{id}/logs/`, a signed-in workspace session, `x-csrf-token`, and their own operation permission. `{id}` is the target/source application ID, not a Langfuse project ID.

| Suffix | JSON request | Successful response |
| --- | --- | --- |
| `otlp/export` | `{ "record_ids": ["RECORD_ID_1", "RECORD_ID_2"] }` | Downloadable `application/json` standard OTLP object; no `ApiSuccess` wrapper |
| `otlp/import` | `{ "otlp": { "resourceSpans": [] } }` | `ApiSuccess<{accepted_events, duplicate_events, record_ids}>` |
| `langfuse/export` | `{ "record_ids": ["RECORD_ID_1", "RECORD_ID_2"], "base_url": "https://YOUR_LANGFUSE_HOST", "public_key": "<PUBLIC_KEY>", "secret_key": "<SECRET_KEY>" }` | `ApiSuccess<{exported_records, exported_spans, trace_ids}>` |
| `langfuse/import` | `{ "trace_ids": ["TRACE_ID_1", "TRACE_ID_2"], "base_url": "https://YOUR_LANGFUSE_HOST", "public_key": "<PUBLIC_KEY>", "secret_key": "<SECRET_KEY>" }` | `ApiSuccess<{accepted_events, duplicate_events, record_ids}>` |

`ApiSuccess` wraps the receipt as `{"data": {...}, "meta": null}`. The empty `resourceSpans` above illustrates the request shape; use a real trace payload to import events. Public SDK ingestion uses the standard OTLP trace response, not this console receipt envelope.

The existing `/api/logs/v1/events` native ingress, collector protocols and native archive behavior remain separate.

## What survives an exchange

1flowbase's own exports embed a versioned original source event in metadata, preserving source/session/task/event identity, raw facts and usage on reimport. This preservation depends on the metadata surviving unchanged. If Langfuse masking or a transformation removes or changes it, native lossless recovery is not guaranteed.

Ordinary external traces may contain only an LLM request's historical Context. Such context is not proof of a complete human conversation or an actual task terminal state. Missing model, usage or completion facts stay missing; imported cost estimates follow the backend's existing pricing rules and do not charge balances. OTLP exchange is not a database backup.

The architecture keeps three responsibilities separate:

```text
OTLP / Langfuse mapping and exchange       protocol layer
                    ↓
source / session / task / event / usage / raw + persistence
                                         unified semantic layer
                    ↑
native collector adapters / OpenLIT       source adapter layer
```

Generic source recovery stays in the shared collector SDK rather than being copied into each plugin. Existing native collectors do not need an upgrade for this exchange feature; collector plugins are unchanged.

The backend protocol layer newly declares `opentelemetry-proto` **0.33.1** for generated Trace messages and their official serde representation, `prost` **0.14.1** for protobuf encoding and decoding, and `tonic-types` **0.14.1** for the official `google.rpc.Status` error representation. Default features are disabled on the protocol dependencies that expose them; no gRPC server or transport, or telemetry provider, is initialized. The OpenTelemetry API or SDK may appear as transitive dependencies of the protocol package, but that does not initialize an instrumentation runtime. OpenLIT **1.15.0** is temporary interoperability test tooling, not a 1flowbase product dependency; the SDK examples above configure your separate client application.

## Optional Network Center routing

Langfuse import and export make outbound requests from the 1flowbase server. If your deployment needs a proxy or another configured egress route, configure **Agent Logs exchange** in **Network Center**; its consumer identifier is `agent_logs_exchange`. This routing configuration is optional and separate from the Langfuse project URL and keys. Local OTLP file exchange does not require a remote Langfuse connection. Incoming OpenLIT traces still use the public HTTP receiver described above.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| SDK upload rejected | Correct application Bearer key, enabled Log reception, exact `/api/logs/otlp/v1/traces` route and supported HTTP content type |
| Console exchange forbidden | Workspace session, CSRF token and the permission for that specific operation; a collector key is insufficient |
| Langfuse exchange fails | Deployment base URL, project key pair, and connectivity from the 1flowbase server |
| No complete conversation after import | Inspect the source spans and metadata; instrumented LLM history may be Context only |
| Fewer records than selected | Compare server receipt and returned IDs; resolve each failed application group before retrying |
| Trace arrives but Metrics fails | This receiver supports Trace only; disable other signals or send them to a supported backend |

## Related guides

- [Native collector protocol and recovery](README.md)
- [Native client integration](native-clients.md)
- [Local Codex / Claude Code collection tutorial](https://github.com/taichuy/1flowbase/wiki/Agent-Logs-Local-Agents-CN)
- [Langfuse OpenTelemetry integration](https://langfuse.com/docs/opentelemetry/get-started)
- [OpenLIT SDK source](https://github.com/openlit/openlit/tree/main/sdk)

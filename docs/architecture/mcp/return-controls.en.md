# MCP return controls and native content projection

[中文](return-controls.md)

Tool `max_inline_chars` sets the serialized JSON text budget (4,000 characters by default). `response_fields` and `string_ranges` select metadata fields and Unicode character ranges; cached detail can be read through `mcp_result` without repeating the business operation. See the Chinese companion for the full control contract.

Managed operations may declare `mcp.output_mapping`:

```json
{
  "mcp_content": [{"type": "image", "data_path": "/data", "mime_type_path": "/mime_type"}],
  "structured_content_path": "/metadata"
}
```

Paths are explicit JSON Pointers into the business response. A block's paths must be valid, distinct, non-overlapping, and separate from the metadata object. The host extracts declared content without inferring it from stdout. Missing paths, non-object metadata, malformed mappings or invalid native fields fail explicitly. The existing image declaration and business response wire format remain supported.

| `type` | Required paths | Optional paths |
| --- | --- | --- |
| `text` | `text_path` | `annotations_path` |
| `image`, `audio` | `data_path`, `mime_type_path` | `annotations_path` |
| `resource_link` | `uri_path`, `name_path` | `mime_type_path`, `description_path`, `annotations_path` |
| `resource` | `uri_path`, exactly one of `text_path` / `blob_path` | `mime_type_path`, `annotations_path` |

Image and audio data must be nonempty canonical base64, with valid `image/*` or `audio/*` MIME types respectively; there is no business MIME allowlist. Empty resource blobs are valid files. Resource URIs must be absolute; the host neither downloads nor resolves them. Embedded resources emit `{type:"resource",resource:{uri,mimeType?,text|blob}}`. Legal annotations are retained. `resource_read` defaults to false and requires at least one embedded resource and permits accompanying resource links; mappings with text, image or audio cannot enable it; it declares eligibility for the resource-read adapter.

Locally mapped native content, including text, is retained independently of metadata selection, text pagination and continuation storage. `max_inline_chars`, field/range selection and cached detail apply only to the separate metadata object. A low metadata budget never truncates a native block. Upstream proxy text keeps its existing metadata-budget behavior, while non-text content survives paging.

The final 256 KiB JSON-RPC envelope limit exempts only validated `image.data`, `audio.data`, embedded `resource.resource.blob`, and `resources/read` result `contents[].blob`. Validation uses a fixed decode buffer without a second full blob allocation. MIME types, URIs, annotations, resource text, metadata, unknown fields and JSON framing all remain budgeted. Invalid binary fields receive no exemption.

The business `output_schema` remains unchanged. `mcp_get.result_schema` projects the actual declared metadata subschema; an unresolved schema remains unconstrained rather than inventing fields. Omitting `mcp.output_mapping` preserves existing text projection.

## Resource rereads

The server advertises `capabilities.resources = {}` alongside tools while retaining protocol version `2025-03-26`. `resources/list` returns `{resources:[]}`: resources produced by tool calls do not imply a directory entry.

For an InterfaceWrapper mapping with `resource_read:true`, embedded resources and links pointing to the same original URI receive a host URI, `1flowbase-resource://read/<BASE64URL_JSON>`. General external links remain unchanged and are never fetched automatically. The self-contained versioned reference contains user/workspace, selected instance, tool ID, current `des_id`, caller business arguments and original resource URI. It stores no response binary, request headers, session credentials or hidden/default bindings and requires no URI registry or binary cache. This declaration is for file reads whose caller arguments are safe to expose as locators.

`resources/read` accepts `{uri}` and returns `{contents:[{uri,mimeType?,text|blob}]}`. The requested host URI remains the returned URI, without recursive wrapping. A reference is a locator, not authorization: each read checks the current user/workspace, selected and visible tool/instance, explicit opt-in and current revision before dispatch. Revoked visibility, disabled opt-in, changed revision, malformed or unknown host URI fail without calling a tool. Editing locator arguments does not grant additional access.

The RPC uses the existing invocation Kernel and ToolCall port, rebuilds current managed discovery and executes the original tool through the same interface dispatch and host executable checks. Only matching embedded resources from a successful call are returned; no partial result is returned on failure. Reads repeat the original business read and can observe a changed file. Both encoding and decoding obey the existing 2 MiB tool-arguments transport budget, with strict versioned payload fields.

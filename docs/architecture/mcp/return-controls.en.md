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

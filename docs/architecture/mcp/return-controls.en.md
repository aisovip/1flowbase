# MCP return controls and native image projection

[中文](return-controls.md)

Tool `max_inline_chars` sets the serialized JSON text budget (4,000 characters by default). `response_fields` and `string_ranges` select metadata fields and Unicode character ranges; cached detail can be read through `mcp_result` without repeating the business operation. See the Chinese companion for the full control contract.

Managed operations may declare `mcp.output_mapping`:

```json
{
  "mcp_content": [{"type": "image", "data_path": "/data", "mime_type_path": "/mime_type"}],
  "structured_content_path": "/metadata"
}
```

Paths are explicit JSON Pointers into the business response object. Image data and metadata paths must not overlap. The host does not infer images from stdout. Missing paths, non-object metadata, empty or malformed base64, unsupported MIME types and malformed mappings fail explicitly. JPEG, PNG, GIF and WebP are supported. Complete base64 and `mimeType` appear in native MCP `image` content; image bytes never enter structuredContent, text pagination or ephemeral continuation storage. Text budgets and field/range selection apply only to metadata, which retains the existing continuation behavior. A low text budget never truncates an image. Native non-text upstream proxy content is also retained when its metadata is paged.

The business `output_schema` remains unchanged. `mcp_get.result_schema` projects the actual declared metadata subschema; an unresolved schema remains unconstrained rather than inventing fields. Omitting `mcp.output_mapping` preserves existing text projection.

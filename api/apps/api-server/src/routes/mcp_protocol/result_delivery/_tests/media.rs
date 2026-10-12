use super::*;

fn declaration() -> Value {
    json!({"mcp_content":[{"type":"image","data_path":"/data","mime_type_path":"/mime_type"}],"structured_content_path":"/metadata"})
}

#[test]
fn image_projection_retains_complete_base64_and_only_structured_metadata() {
    let data = STANDARD.encode(vec![42; 64_000]);
    let metadata = json!({"path":"/tmp/image.jpg","byte_count":64000});
    let mapped = project(
        &mapping(&declaration()).unwrap().unwrap(),
        &json!({"data":data,"mime_type":"image/jpeg","metadata":metadata}),
    )
    .unwrap();
    assert_eq!(mapped.content[0]["data"], data);
    assert_eq!(mapped.content[0]["mimeType"], "image/jpeg");
    assert_eq!(mapped.structured_content, Some(metadata.clone()));
    assert_eq!(
        STANDARD
            .decode(mapped.content[0]["data"].as_str().unwrap())
            .unwrap()
            .len(),
        64000
    );
    let delivered = retain_content(super::super::tool_result(metadata), &mapped.content);
    assert_eq!(delivered["content"][1]["data"], data);
    assert!(delivered["structuredContent"].get("data").is_none());
}

#[test]
fn media_projection_rejects_invalid_data_paths_mime_and_mapping() {
    let parsed = mapping(&declaration()).unwrap().unwrap();
    for source in [
        json!({"data":"%%%","mime_type":"image/jpeg","metadata":{}}),
        json!({"data":"","mime_type":"image/jpeg","metadata":{}}),
        json!({"data":"YQ==","mime_type":"text/plain","metadata":{}}),
        json!({"mime_type":"image/jpeg","metadata":{}}),
        json!({"data":"YQ==","mime_type":"image/jpeg","metadata":[]}),
    ] {
        assert!(project(&parsed, &source).is_err());
    }
    assert!(mapping(&json!({"mcp_content":[]})).is_err());
    assert!(mapping(&json!({"properties":{"stdout":{"type":"string"}}}))
        .unwrap()
        .is_none());
    for mime in ["image/jpeg", "image/png", "image/gif", "image/webp"] {
        assert!(project(
            &parsed,
            &json!({"data":"YQ==","mime_type":mime,"metadata":{}})
        )
        .is_ok());
    }
}

#[test]
fn proxy_media_survives_metadata_paging_without_duplicating_original_text() {
    let upstream =
        json!([{"type":"image","data":"YQ==","mimeType":"image/png"},{"type":"text","text":"old"}]);
    let delivered = retain_content(
        super::super::tool_result(json!({"next_cursor":"cursor"})),
        &upstream,
    );
    assert_eq!(delivered["content"].as_array().unwrap().len(), 2);
    assert_eq!(delivered["content"][1], upstream[0]);
    assert_ne!(delivered["content"][0]["text"], "old");
}

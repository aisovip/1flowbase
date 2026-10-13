use super::normalize_preview_policy;
use serde_json::json;

#[test]
fn local_policy_defaults_preserve_other_settings_and_reject_non_boolean() {
    assert_eq!(
        normalize_preview_policy(json!({"description":"keep"}), "local").unwrap(),
        json!({"description":"keep","image_preview_enabled":true})
    );
    assert_eq!(
        normalize_preview_policy(json!({"image_preview_enabled":false}), "local").unwrap(),
        json!({"image_preview_enabled":false})
    );
    assert_eq!(
        normalize_preview_policy(json!({}), "s3").unwrap(),
        json!({})
    );
    for value in [json!(null), json!("false"), json!(0)] {
        assert!(normalize_preview_policy(json!({"image_preview_enabled":value}), "local").is_err());
    }
    assert!(normalize_preview_policy(json!([]), "local").is_err());
}

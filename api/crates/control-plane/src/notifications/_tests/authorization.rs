use super::*;

#[test]
fn host_secret_projection_rejects_existing_and_new_credentials_and_patch_deletion_is_exact() {
    let mut descriptor = system_notification_descriptor();
    descriptor.config_form=serde_json::from_value(json!({"schema_version":"1.0.0","fields":[{"key":"token","label":"Token","type":"string","control":"password"}]})).unwrap();
    let config = json!({"token":"private-config-token"});
    let managed = BTreeMap::from([
        ("bot_token".into(), json!("private-bot-token")),
        ("cursor".into(), json!({"opaque":"private-cursor"})),
    ]);
    let patch = BTreeMap::from([("context_token".into(), json!("private-context-token"))]);
    for secret in [
        "private-config-token",
        "private-bot-token",
        "private-cursor",
        "private-context-token",
    ] {
        assert!(public_contains_credentials(
            &json!({"result":{"delivery_id":format!("sent:{secret}")}}),
            &descriptor,
            &config,
            &managed,
            &patch
        ));
        assert!(public_contains_credentials(
            &json!({"user_action":{"qr_code_url":format!("https://example.com/{secret}")}}),
            &descriptor,
            &config,
            &managed,
            &patch
        ));
    }
    assert!(!public_contains_credentials(
        &json!({"status":"authorized"}),
        &descriptor,
        &config,
        &managed,
        &patch
    ));
    let mut merged = managed.clone();
    apply_patch(
        &mut merged,
        BTreeMap::from([
            ("bot_token".into(), Value::Null),
            ("context_token".into(), json!("private-context-token")),
        ]),
    );
    assert!(!merged.contains_key("bot_token"));
    assert_eq!(merged["cursor"], managed["cursor"]);
    assert_eq!(merged["context_token"], patch["context_token"]);
}

#[test]
fn encrypted_cached_qr_url_is_public_only_in_the_exact_authorization_display_field() {
    let descriptor = system_notification_descriptor();
    let cached = json!("https://example.com/display-qr");
    let secrets = BTreeMap::from([("qr_code_url".into(), cached.clone())]);
    let empty = BTreeMap::new();
    let pending = json!({"status":"pending","user_action":{"kind":"qr_code","qr_code_url":cached,"poll_interval_seconds":2}});
    assert!(!public_contains_credentials(
        &pending,
        &descriptor,
        &json!({}),
        &secrets,
        &empty
    ));
    assert!(!public_contains_credentials(
        &json!({"authorization":pending,"result":null}),
        &descriptor,
        &json!({}),
        &empty,
        &secrets
    ));
    assert!(public_contains_credentials(
        &json!({"authorization":pending,"result":{"delivery_id":cached}}),
        &descriptor,
        &json!({}),
        &secrets,
        &empty
    ));
    let mut invalid = pending.clone();
    invalid["user_action"]["expires_at"] = cached.clone();
    assert!(public_contains_credentials(
        &invalid,
        &descriptor,
        &json!({}),
        &secrets,
        &empty
    ));
    // The QR-field exception never exempts the same string when classified as a bot token.
    let token = BTreeMap::from([("bot_token".into(), cached)]);
    assert!(public_contains_credentials(
        &pending,
        &descriptor,
        &json!({}),
        &token,
        &empty
    ));
}

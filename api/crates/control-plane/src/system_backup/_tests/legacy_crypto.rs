use super::*;
use crate::system_backup::{recover_source_master_key, resolve_backup_key};

fn historical_backup() -> (serde_json::Value, SealedBackupManifest) {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test-fixtures/legacy-crypto/fixtures.json"
    )))
    .unwrap();
    let backup = fixture["backup"].clone();
    let sealed = serde_json::from_value(backup["sealed_manifest"].clone()).unwrap();
    (backup, sealed)
}

#[tokio::test]
async fn pre_upgrade_password_backup_recovers_key_and_authenticates_original_manifest() {
    let (fixture, sealed) = historical_backup();
    let password = fixture["password"].as_str().unwrap();
    let key = resolve_backup_key(&RejectingKeyProvider, sealed.manifest(), Some(password))
        .await
        .unwrap();
    assert_eq!(
        key.expose_bytes(),
        STANDARD
            .decode(fixture["key_base64"].as_str().unwrap())
            .unwrap()
    );
    assert_eq!(
        recover_source_master_key(&RejectingKeyProvider, sealed.manifest(), Some(password))
            .await
            .unwrap(),
        fixture["source_master_key"].as_str().unwrap()
    );
    verify_backup_manifest(&sealed, &key).unwrap();
    assert!(
        resolve_backup_key(&RejectingKeyProvider, sealed.manifest(), None)
            .await
            .is_err()
    );
    assert!(resolve_backup_key(
        &RejectingKeyProvider,
        sealed.manifest(),
        Some("wrong-password")
    )
    .await
    .is_err());
    assert!(recover_source_master_key(
        &RejectingKeyProvider,
        sealed.manifest(),
        Some("wrong-password")
    )
    .await
    .is_err());
    assert!(verify_backup_manifest(&sealed, &super::key()).is_err());
    let mut tampered = serde_json::to_value(&sealed).unwrap();
    tampered["manifest"]["application_build"] = serde_json::json!("tampered.build");
    let tampered: SealedBackupManifest = serde_json::from_value(tampered).unwrap();
    assert!(verify_backup_manifest(&tampered, &key).is_err());
}

#[tokio::test]
async fn pre_upgrade_backup_envelope_preserves_bytes_and_rejects_wrong_identity_and_tamper() {
    let (fixture, sealed) = historical_backup();
    let key = resolve_backup_key(
        &RejectingKeyProvider,
        sealed.manifest(),
        fixture["password"].as_str(),
    )
    .await
    .unwrap();
    let original = STANDARD
        .decode(fixture["encrypted_stream_base64"].as_str().unwrap())
        .unwrap();
    let expected = STANDARD
        .decode(fixture["plaintext_base64"].as_str().unwrap())
        .unwrap();
    let manifest = sealed.manifest();
    let component = &manifest.components()[0];
    let mut plaintext = Vec::new();
    let receipt = decrypt_backup_stream(
        &original[..],
        &mut plaintext,
        &key,
        manifest.backup_set_id(),
        &component.component_id,
    )
    .await
    .unwrap();
    assert_eq!(plaintext, expected);
    assert_eq!(receipt.plaintext_size_bytes, component.size_bytes);
    assert_eq!(receipt.plaintext_digest, component.content_digest);
    assert_eq!(&receipt.envelope_digest, manifest.envelope_digest());
    let wrong_component = BackupComponentId::try_from("postgres/other").unwrap();
    let wrong_key = super::key();
    for (bytes, candidate_key, set_id, component_id) in [
        (
            original.clone(),
            &wrong_key,
            manifest.backup_set_id(),
            component.component_id.clone(),
        ),
        (
            original.clone(),
            &key,
            BackupSetId::new(),
            component.component_id.clone(),
        ),
        (
            original.clone(),
            &key,
            manifest.backup_set_id(),
            wrong_component,
        ),
        (
            {
                let mut bytes = original.clone();
                *bytes.last_mut().unwrap() ^= 1;
                bytes
            },
            &key,
            manifest.backup_set_id(),
            component.component_id.clone(),
        ),
        (
            original[..original.len() - 1].to_vec(),
            &key,
            manifest.backup_set_id(),
            component.component_id.clone(),
        ),
        (
            {
                let mut bytes = original.clone();
                bytes.push(0);
                bytes
            },
            &key,
            manifest.backup_set_id(),
            component.component_id.clone(),
        ),
    ] {
        assert!(decrypt_backup_stream(
            &bytes[..],
            &mut Vec::new(),
            candidate_key,
            set_id,
            &component_id
        )
        .await
        .is_err());
    }
}

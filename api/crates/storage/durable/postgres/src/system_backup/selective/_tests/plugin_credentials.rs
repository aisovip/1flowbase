use super::*;
use crate::{
    plugin_credential_repository::{credential_associated_data, PgPluginCredentialRepository},
    secret_crypto::{decrypt_secret_json_with_aad, encrypt_secret_json_with_aad},
};
use control_plane_contracts::ports::PluginCredentialRepository;
use extension_contracts::{PluginCredentialBinding, PluginCredentialRequest};
use serde_json::json;

#[test]
fn credential_backup_rekey_preserves_identity_and_rejects_wrong_keys_or_tampering() {
    let scope = Uuid::now_v7().to_string();
    let aad = credential_associated_data("acme|publisher", "ssh", &scope, "host|1").unwrap();
    let plaintext = json!({"password":"backup-fixture-secret"});
    let original = json!({
        "publisher_namespace":"acme|publisher", "plugin_code":"ssh", "scope_id":scope,
        "credential_id":"host|1", "encrypted_value":encrypt_secret_json_with_aad(&plaintext,"source",&aad).unwrap()
    });
    let mut unchanged = original.clone();
    validation::rekey("plugin_credentials", &mut unchanged, "source", "source").unwrap();
    assert_eq!(unchanged, original);
    let mut rekeyed = original.clone();
    validation::rekey("plugin_credentials", &mut rekeyed, "source", "target").unwrap();
    assert_eq!(
        decrypt_secret_json_with_aad(&rekeyed["encrypted_value"], "target", &aad).unwrap(),
        plaintext
    );
    assert!(decrypt_secret_json_with_aad(&rekeyed["encrypted_value"], "source", &aad).is_err());
    assert!(!rekeyed.to_string().contains("backup-fixture-secret"));
    assert!(validation::rekey(
        "plugin_credentials",
        &mut original.clone(),
        "wrong",
        "target"
    )
    .is_err());
    for field in [
        "publisher_namespace",
        "plugin_code",
        "scope_id",
        "credential_id",
    ] {
        let mut tampered = original.clone();
        tampered[field] = json!("changed");
        assert!(
            validation::rekey("plugin_credentials", &mut tampered, "source", "target").is_err(),
            "{field}"
        );
    }
}

#[tokio::test]
async fn selective_extension_configuration_roundtrips_credentials_under_the_target_key() {
    let (db, _) = fixture().await;
    let repo = PgSelectiveBackupRepository::new(db.clone());
    let credentials = PgPluginCredentialRepository::new(db.clone(), "source").unwrap();
    let binding = PluginCredentialBinding {
        contribution_id: "ssh.settings".into(),
        installation_id: Uuid::now_v7().to_string(),
        publisher_namespace: "acme".into(),
        plugin_code: "ssh".into(),
        plugin_version: "1.0.0".into(),
        scope_id: domain::SYSTEM_SCOPE_ID.to_string(),
        deadline_unix_ms: i64::MAX,
    };
    let plaintext = json!({"password":"backup-fixture-secret"});
    credentials
        .execute_plugin_credential(
            &binding,
            &PluginCredentialRequest::Put {
                credential_id: "host-1".into(),
                value: plaintext.clone(),
            },
        )
        .await
        .unwrap();
    let categories = repo.catalog().await.unwrap();
    let category = categories
        .iter()
        .find(|c| c.feature_id == "system.extension-center")
        .unwrap();
    assert!(category
        .structure_tables
        .contains(&"plugin_credentials".into()));
    assert!(!category.data_tables.contains(&"plugin_credentials".into()));
    let bytes = capture(&repo, select("extension-center", true, false)).await;
    let wrong_key = repo
        .preflight(reader(bytes.clone()), "wrong", "target")
        .await
        .unwrap();
    assert!(!wrong_key.failures.is_empty());
    let preview = repo
        .preflight(reader(bytes.clone()), "source", "target")
        .await
        .unwrap();
    assert!(preview.failures.is_empty(), "{:?}", preview.failures);
    assert!(preview
        .selected_tables
        .contains(&"plugin_credentials".into()));
    sqlx::query("delete from plugin_credentials")
        .execute(&db)
        .await
        .unwrap();
    repo.restore(reader(bytes), "source", "target", true)
        .await
        .unwrap();
    let get = PluginCredentialRequest::Get {
        credential_id: "host-1".into(),
    };
    let target = PgPluginCredentialRepository::new(db.clone(), "target").unwrap();
    assert_eq!(
        target
            .execute_plugin_credential(&binding, &get)
            .await
            .unwrap()
            .value,
        Some(plaintext)
    );
    assert!(credentials
        .execute_plugin_credential(&binding, &get)
        .await
        .is_err());
    let encrypted: serde_json::Value =
        sqlx::query_scalar("select encrypted_value from plugin_credentials")
            .fetch_one(&db)
            .await
            .unwrap();
    assert!(!encrypted.to_string().contains("backup-fixture-secret"));
}

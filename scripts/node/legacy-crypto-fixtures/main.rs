//! Offline public test data only. Runtime modules below are copied verbatim from BASELINE.
use base64::{engine::general_purpose::STANDARD, Engine as _};
use domain::{
    ApplicationBuild, ArtifactRebuildability, BackupComponent, BackupComponentDisposition,
    BackupComponentId, BackupComponentKind, BackupComponentRestoreTarget, BackupManifest,
    BackupSetId, BackupSourceIdentity, ContentDigest, KeyFingerprint, MigrationHead,
};
use ed25519_dalek::{pkcs8::EncodePublicKey, Signer, SigningKey};
use serde_json::json;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

mod envelope;
mod password;
mod protection;
mod secret;
mod ports {
    pub use control_plane_contracts::ports::{BackupKeyMaterial, BackupKeyProviderError};
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let output = std::env::args().nth(1).expect("fixture output path");
    let password = "legacy-storage-fixture-password";
    let phc = password::generate(password)?;
    let master_key = "legacy-provider-master-key-public-test-data";
    let secret_plaintext =
        json!({"bot_token":"legacy-public-fixture-token","bound_user_id":"legacy-user"});
    let aad = b"1flowbase/legacy-crypto-fixture/scope-v1";
    let encrypted_secret = secret::encrypt_secret_json(&secret_plaintext, master_key)?;
    let encrypted_secret_aad =
        secret::encrypt_secret_json_with_aad(&secret_plaintext, master_key, aad)?;

    let backup_password = "legacy-backup-fixture-password";
    let source_master_key = "legacy-source-master-public-fixture";
    let protected = protection::password_protection(
        backup_password,
        Some(&STANDARD.encode(source_master_key)),
    )?;
    let backup_set_id = BackupSetId::from_uuid("0199bcde-1234-7000-8000-000000000001".parse()?);
    let component_id = BackupComponentId::try_from("postgres/main")?;
    let plaintext = b"historical backup data\0\xff\x80with binary bytes";
    let mut encrypted_stream = Vec::new();
    let receipt = envelope::encrypt_backup_stream(
        &plaintext[..],
        &mut encrypted_stream,
        &protected.key,
        backup_set_id,
        &component_id,
    )
    .await?;
    let manifest = BackupManifest::try_new_password_encrypted(
        backup_set_id,
        OffsetDateTime::UNIX_EPOCH,
        ApplicationBuild::try_from("legacy.crypto.fixture")?,
        MigrationHead::try_from("migration.legacy.crypto.fixture")?,
        KeyFingerprint::try_from(format!(
            "{:x}",
            Sha256::digest(source_master_key.as_bytes())
        ))?,
        protected.key.fingerprint().clone(),
        protected.salt_base64,
        protected.encrypted_source_master_key_base64,
        vec![BackupComponent {
            component_id,
            kind: BackupComponentKind::PostgreSql,
            source_identity: BackupSourceIdentity::try_from("postgres/main")?,
            content_type: "application/octet-stream".into(),
            size_bytes: receipt.plaintext_size_bytes,
            content_digest: receipt.plaintext_digest,
            disposition: BackupComponentDisposition::Embedded,
            rebuildability: ArtifactRebuildability::NotApplicable,
            restore_target: BackupComponentRestoreTarget::PostgreSql,
        }],
        receipt.plaintext_size_bytes,
        receipt.envelope_digest,
    )?;
    let sealed = envelope::authenticate_backup_manifest(manifest, &protected.key)?;

    let artifact = br#"{"template_id":"legacy-crypto-fixture","version":1}"#;
    let signing = SigningKey::from_bytes(&[23; 32]);
    let wrong_signing = SigningKey::from_bytes(&[24; 32]);
    let line_ending = ed25519_dalek::pkcs8::spki::der::pem::LineEnding::LF;
    let fixtures = json!({
        "schema_version": 1,
        "baseline": "6a0530638c5839ed19893a16fca3a4b890af8d4e",
        "password": {"plaintext":password,"phc":phc},
        "secret": {"master_key":master_key,"plaintext":secret_plaintext,"encrypted":encrypted_secret,
            "aad_base64":STANDARD.encode(aad),"encrypted_with_aad":encrypted_secret_aad},
        "backup": {"password":backup_password,"source_master_key":source_master_key,
            "key_base64":STANDARD.encode(protected.key.expose_bytes()),
            "plaintext_base64":STANDARD.encode(plaintext),"encrypted_stream_base64":STANDARD.encode(encrypted_stream),
            "sealed_manifest":sealed},
        "artifact": {"bytes_base64":STANDARD.encode(artifact),"sha256":format!("{:x}",Sha256::digest(artifact)),
            "algorithm":"ed25519","key_id":"legacy-fixture-key",
            "public_key_pem":signing.verifying_key().to_public_key_pem(line_ending)?,
            "wrong_public_key_pem":wrong_signing.verifying_key().to_public_key_pem(line_ending)?,
            "signature_base64":STANDARD.encode(signing.sign(artifact).to_bytes())},
    });
    std::fs::write(
        output,
        format!("{}\n", serde_json::to_string_pretty(&fixtures)?),
    )?;
    Ok(())
}

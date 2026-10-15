use super::*;
use base64::{engine::general_purpose::STANDARD, Engine as _};

#[test]
fn pre_upgrade_stored_secrets_decrypt_and_reject_wrong_key_context_and_tamper() {
    let fixture: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../../test-fixtures/legacy-crypto/fixtures.json"
    )))
    .unwrap();
    let secret = &fixture["secret"];
    let key = secret["master_key"].as_str().unwrap();
    let aad = STANDARD
        .decode(secret["aad_base64"].as_str().unwrap())
        .unwrap();
    assert_eq!(
        decrypt_secret_json(&secret["encrypted"], key).unwrap(),
        secret["plaintext"]
    );
    assert_eq!(
        decrypt_secret_json_with_aad(&secret["encrypted_with_aad"], key, &aad).unwrap(),
        secret["plaintext"]
    );
    assert!(decrypt_secret_json(&secret["encrypted"], "wrong-public-test-key").is_err());
    assert!(
        decrypt_secret_json_with_aad(&secret["encrypted_with_aad"], key, b"wrong-scope").is_err()
    );
    assert!(decrypt_secret_json(&secret["encrypted_with_aad"], key).is_err());
    for name in ["encrypted", "encrypted_with_aad"] {
        let mut tampered = secret[name].clone();
        let mut ciphertext = tampered["ciphertext"].as_str().unwrap().to_owned();
        let replacement = if ciphertext.starts_with('0') {
            "1"
        } else {
            "0"
        };
        ciphertext.replace_range(..1, replacement);
        tampered["ciphertext"] = json!(ciphertext);
        let context = if name == "encrypted" {
            &[][..]
        } else {
            &aad[..]
        };
        assert!(decrypt_secret_json_with_aad(&tampered, key, context).is_err());
    }
}

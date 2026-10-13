use super::*;
use crate::secret_crypto::{decrypt_secret_json_with_aad, encrypt_secret_json_with_aad};
use serde_json::json;

#[test]
fn notification_rekey_preserves_bound_identity_and_never_accepts_wrong_keys() {
    let workspace = Uuid::now_v7();
    let instance = Uuid::now_v7();
    let aad = crate::notification_repository::aad(workspace, instance);
    let plain = json!({"token":"notification-backup-secret"});
    let original = json!({
        "id":instance,"workspace_id":workspace,
        "encrypted_secret_json":encrypt_secret_json_with_aad(&plain,"source",&aad).unwrap()
    });
    let mut same = original.clone();
    validation::rekey("notification_instances", &mut same, "source", "source").unwrap();
    assert_eq!(same, original);
    let mut rekeyed = original.clone();
    validation::rekey("notification_instances", &mut rekeyed, "source", "target").unwrap();
    assert_eq!(
        decrypt_secret_json_with_aad(&rekeyed["encrypted_secret_json"], "target", &aad).unwrap(),
        plain
    );
    assert!(
        decrypt_secret_json_with_aad(&rekeyed["encrypted_secret_json"], "source", &aad).is_err()
    );
    assert!(!rekeyed.to_string().contains("notification-backup-secret"));
    assert!(validation::rekey(
        "notification_instances",
        &mut original.clone(),
        "wrong",
        "target"
    )
    .is_err());
    for field in ["id", "workspace_id"] {
        let mut tampered = original.clone();
        tampered[field] = json!(Uuid::now_v7());
        assert!(
            validation::rekey("notification_instances", &mut tampered, "source", "target").is_err()
        );
    }
    let mut builtin = json!({"encrypted_secret_json":null});
    validation::rekey("notification_instances", &mut builtin, "source", "target").unwrap();
    assert_eq!(builtin, json!({"encrypted_secret_json":null}));
}

#[tokio::test]
async fn selective_notification_configuration_roundtrips_credentials_without_inbox_history() {
    let (db, actor) = fixture().await;
    let store = crate::PgControlPlaneStore::new(db.clone());
    let tenant = store.upsert_root_tenant().await.unwrap();
    let workspace = store
        .upsert_workspace(tenant.id, "Notification backup")
        .await
        .unwrap();
    let instance = Uuid::now_v7();
    let aad = crate::notification_repository::aad(workspace.id, instance);
    let plain = json!({"token":"notification-backup-secret"});
    let encrypted = encrypt_secret_json_with_aad(&plain, "source", &aad).unwrap();
    sqlx::query("insert into notification_instances(id,workspace_id,type_code,display_name,encrypted_secret_json,configured_secret_fields) values($1,$2,'fixture','Original',$3,'[\"token\"]'::jsonb)")
        .bind(instance).bind(workspace.id).bind(encrypted).execute(&db).await.unwrap();
    let message = Uuid::now_v7();
    sqlx::query("insert into system_notifications(id,workspace_id,instance_id,recipient_user_id,title,content) values($1,$2,$3,$4,'Inbox','Original inbox')")
        .bind(message).bind(workspace.id).bind(instance).bind(actor).execute(&db).await.unwrap();
    let repository = PgSelectiveBackupRepository::new(db.clone());
    let bytes = capture(&repository, select("notifications", true, false)).await;
    let preview = repository
        .preflight(reader(bytes.clone()), "source", "target")
        .await
        .unwrap();
    assert!(preview.failures.is_empty(), "{:?}", preview.failures);
    assert_eq!(
        preview.selected_tables,
        vec!["notification_instances".to_owned()]
    );
    sqlx::query("update notification_instances set display_name='Changed' where id=$1")
        .bind(instance)
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("update system_notifications set content='Target inbox' where id=$1")
        .bind(message)
        .execute(&db)
        .await
        .unwrap();
    repository
        .restore(reader(bytes), "source", "target", true)
        .await
        .unwrap();
    let name: String =
        sqlx::query_scalar("select display_name from notification_instances where id=$1")
            .bind(instance)
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(name, "Original");
    let restored = sqlx::query_scalar::<_, serde_json::Value>(
        "select encrypted_secret_json from notification_instances where id=$1",
    )
    .bind(instance)
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(
        decrypt_secret_json_with_aad(&restored, "target", &aad).unwrap(),
        plain
    );
    let inbox: String = sqlx::query_scalar("select content from system_notifications where id=$1")
        .bind(message)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(inbox, "Target inbox");
}

use control_plane::{
    notifications::{NotificationSenderPort, NotificationService, PatchNotificationInstance},
    ports::{CreateMemberInput, NotificationMessageListInput},
};
use domain::NotificationInstanceStatus;
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn committed_inbox_deduplicates_and_enforces_recipient_workspace_and_management_scope() {
    let (store, workspace, root) = super::support::seed_store().await;
    let actor = store
        .load_actor_context(root.id, workspace.tenant_id, workspace.id, None)
        .await
        .unwrap();
    let member = store
        .create_member_with_default_role(&CreateMemberInput {
            actor_user_id: root.id,
            workspace_id: workspace.id,
            account: "notification-member".into(),
            email: "notification@example.com".into(),
            phone: None,
            password_hash: "hash".into(),
            name: "Member".into(),
            nickname: "Member".into(),
            introduction: String::new(),
            email_login_enabled: true,
            phone_login_enabled: false,
        })
        .await
        .unwrap();
    let member_actor = store
        .load_actor_context(member.id, workspace.tenant_id, workspace.id, None)
        .await
        .unwrap();
    let service = NotificationService::new(store.clone(), "test-notification-master-key".into());
    service.bootstrap().await.unwrap();
    let instances = service.list_instances(&actor).await.unwrap();
    assert_eq!(instances.len(), 1);
    let id = instances[0].record.id;
    assert!(service.list_instances(&member_actor).await.is_err());
    let catalog = service.types(&member_actor).await.unwrap();
    assert!(catalog[0].descriptor.input_form.fields[0]
        .options
        .iter()
        .any(|option| option.value == json!(member.id)));
    let sent = service
        .send(
            &actor,
            id,
            json!({"user_ids":[root.id,member.id,member.id],"title":"Ready","content":"Committed"}),
        )
        .await
        .unwrap();
    assert_eq!(sent.result["recipient_count"], 2);
    assert_eq!(service.unread_count(&actor).await.unwrap(), 1);
    assert_eq!(service.unread_count(&member_actor).await.unwrap(), 1);
    let root_page = service
        .messages(&actor, NotificationMessageListInput::default())
        .await
        .unwrap();
    let member_page = service
        .messages(&member_actor, NotificationMessageListInput::default())
        .await
        .unwrap();
    assert_eq!(member_page.items.len(), 1);
    assert!(service
        .read(&member_actor, root_page.items[0].id)
        .await
        .is_err());
    let read = service
        .read(&member_actor, member_page.items[0].id)
        .await
        .unwrap();
    assert_eq!(
        service.read(&member_actor, read.id).await.unwrap().read_at,
        read.read_at
    );
    assert_eq!(service.read_all(&member_actor).await.unwrap(), 0);
    assert_eq!(service.read_all(&actor).await.unwrap(), 1);
    for input in [
        json!({"user_ids":[],"title":"t","content":"c"}),
        json!({"user_ids":[Uuid::now_v7()],"title":"t","content":"c"}),
        json!({"user_ids":[root.id],"title":"","content":"c"}),
    ] {
        assert!(service.send(&actor, id, input).await.is_err());
    }
    assert_eq!(
        service
            .messages(&actor, Default::default())
            .await
            .unwrap()
            .items
            .len(),
        1
    );
    service
        .patch_instance(
            &actor,
            id,
            PatchNotificationInstance {
                status: Some(NotificationInstanceStatus::Disabled),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(service
        .send(
            &actor,
            id,
            json!({"user_ids":[root.id],"title":"t","content":"c"})
        )
        .await
        .is_err());
    let other = store
        .upsert_workspace(workspace.tenant_id, "Other notification workspace")
        .await
        .unwrap();
    let foreign_actor = store
        .load_actor_context(root.id, workspace.tenant_id, other.id, None)
        .await
        .unwrap();
    assert!(service.get_instance(&foreign_actor, id).await.is_err());
    assert!(service
        .read(&foreign_actor, root_page.items[0].id)
        .await
        .is_err());
    assert_eq!(service.unread_count(&foreign_actor).await.unwrap(), 0);
    service.delete_instance(&actor, id).await.unwrap();
    assert_eq!(
        service
            .messages(&actor, Default::default())
            .await
            .unwrap()
            .items
            .len(),
        1
    );
    assert!(service
        .send(
            &actor,
            id,
            json!({"user_ids":[root.id],"title":"t","content":"c"})
        )
        .await
        .is_err());
}

#[tokio::test]
async fn credentials_are_encrypted_bound_to_instance_and_excluded_from_public_records() {
    use control_plane::ports::{NotificationRepository, SaveNotificationInstanceInput};
    let (store, workspace, _) = super::support::seed_store().await;
    let now = time::OffsetDateTime::now_utc();
    let id = Uuid::now_v7();
    let record = domain::NotificationInstanceRecord {
        id,
        workspace_id: workspace.id,
        type_code: "fixture/provider/email".into(),
        installation_id: Some(Uuid::now_v7()),
        display_name: "Email".into(),
        status: NotificationInstanceStatus::Enabled,
        config_json: json!({"endpoint":"https://example.test"}),
        configured_secret_fields: vec!["token".into()],
        created_at: now,
        updated_at: now,
    };
    let key = "notification-fixture-master-key";
    let saved = store
        .save_notification_instance(SaveNotificationInstanceInput {
            record,
            secret_config_json: json!({"token":"private-token"}),
            master_key: key.into(),
            create: true,
        })
        .await
        .unwrap();
    assert!(!serde_json::to_string(&saved)
        .unwrap()
        .contains("private-token"));
    let encrypted: serde_json::Value =
        sqlx::query_scalar("SELECT encrypted_secret_json FROM notification_instances WHERE id=$1")
            .bind(id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert!(!encrypted.to_string().contains("private-token"));
    assert_eq!(
        store
            .resolve_notification_secret_config(workspace.id, id, key)
            .await
            .unwrap(),
        json!({"token":"private-token"})
    );
    assert!(store
        .resolve_notification_secret_config(workspace.id, id, "wrong-key")
        .await
        .is_err());
    let other_id = Uuid::now_v7();
    sqlx::query("INSERT INTO notification_instances (id,workspace_id,type_code,display_name,encrypted_secret_json) VALUES ($1,$2,'system','copied',$3)").bind(other_id).bind(workspace.id).bind(encrypted).execute(store.pool()).await.unwrap();
    assert!(store
        .resolve_notification_secret_config(workspace.id, other_id, key)
        .await
        .is_err());
}

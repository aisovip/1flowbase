//! Expected behavior: recipient/workspace isolation, filtered latest sorting, channel-only
//! reads, reversible read state, bounded pagination, and retained deleted-instance history.
use control_plane::{
    notifications::{CreateNotificationInstance, NotificationSenderPort, NotificationService},
    ports::{NotificationChannelListInput, NotificationMessageListInput},
};
use domain::{NotificationReadStatus, NotificationSource};
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn channel_projection_and_read_commands_preserve_recipient_history() {
    let (store, workspace, root) = super::support::seed_store().await;
    let actor = store
        .load_actor_context(root.id, workspace.tenant_id, workspace.id, None)
        .await
        .unwrap();
    let service = NotificationService::new(store.clone(), "test-master-key".into());
    service.bootstrap().await.unwrap();
    let first = service.list_instances(&actor).await.unwrap()[0].record.id;
    let second = service
        .create_instance(
            &actor,
            CreateNotificationInstance {
                type_code: "system".into(),
                installation_id: None,
                display_name: "Second channel".into(),
                config_json: json!({}),
                status: None,
            },
        )
        .await
        .unwrap()
        .record
        .id;
    for (id, title) in [(first, "Older"), (second, "Newest")] {
        service
            .send(
                &actor,
                NotificationSource::Workflow,
                id,
                json!({"user_ids":[root.id],"title":title,"content":"Body"}),
            )
            .await
            .unwrap();
    }
    let channels = service
        .channels(
            &actor,
            NotificationChannelListInput {
                page_size: Some(1),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(channels.items.len(), 1);
    assert_eq!(channels.items[0].instance_id, second);
    assert_eq!(channels.items[0].unread_count, 1);
    let next = service
        .channels(
            &actor,
            NotificationChannelListInput {
                cursor: channels.next_cursor,
                page_size: Some(1),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(next.items[0].instance_id, first);
    assert!(next.next_cursor.is_none());
    assert_eq!(service.read_channel(&actor, first).await.unwrap(), 1);
    assert_eq!(service.read_channel(&actor, first).await.unwrap(), 0);
    let unread = service
        .channels(
            &actor,
            NotificationChannelListInput {
                read_status: Some(NotificationReadStatus::Unread),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(unread.items.len(), 1);
    assert_eq!(unread.items[0].instance_id, second);
    let read = service
        .messages(
            &actor,
            NotificationMessageListInput {
                instance_id: Some(first),
                read_status: Some(NotificationReadStatus::Read),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(read.items.len(), 1);
    assert_eq!(read.unread_count, 0);
    assert!(service
        .unread(&actor, read.items[0].id)
        .await
        .unwrap()
        .read_at
        .is_none());
    assert_eq!(service.unread_count(&actor).await.unwrap(), 2);
    assert_eq!(
        service.read_channel(&actor, Uuid::now_v7()).await.unwrap(),
        0
    );
    assert_eq!(service.unread_count(&actor).await.unwrap(), 2);
    service.delete_instance(&actor, first).await.unwrap();
    let history = service.channels(&actor, Default::default()).await.unwrap();
    let deleted = history
        .items
        .iter()
        .find(|c| c.instance_id == first)
        .unwrap();
    assert!(deleted.deleted);
    assert_eq!(deleted.type_code, "system");
    assert_eq!(deleted.display_name, "Deleted notification channel");
    assert_eq!(deleted.latest_title, "Older");
    assert_eq!(service.read_channel(&actor, first).await.unwrap(), 1);
    let other = store
        .upsert_workspace(workspace.tenant_id, "Channel isolation")
        .await
        .unwrap();
    let foreign = store
        .load_actor_context(root.id, workspace.tenant_id, other.id, None)
        .await
        .unwrap();
    assert!(service
        .channels(&foreign, Default::default())
        .await
        .unwrap()
        .items
        .is_empty());
    assert!(service.unread(&foreign, read.items[0].id).await.is_err());
    assert_eq!(service.read_channel(&foreign, second).await.unwrap(), 0);
    assert!(service
        .channels(
            &actor,
            NotificationChannelListInput {
                page_size: Some(101),
                ..Default::default()
            }
        )
        .await
        .is_err());
    assert!(service
        .channels(
            &actor,
            NotificationChannelListInput {
                cursor: Some("invalid".into()),
                ..Default::default()
            }
        )
        .await
        .is_err());
    let logs = service.logs(&actor, Default::default()).await.unwrap();
    assert!(logs
        .items
        .iter()
        .all(|l| l.source == NotificationSource::Workflow));
}

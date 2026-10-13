use crate::{
    secret_crypto::{decrypt_secret_json_with_aad, encrypt_secret_json_with_aad},
    PgControlPlaneStore,
};
use anyhow::Result;
use async_trait::async_trait;
use control_plane_contracts::{
    ports::{NotificationMessageListInput, NotificationRepository, SaveNotificationInstanceInput},
    ControlPlaneContractError as Error,
};
use domain::{
    NotificationInstanceRecord, NotificationInstanceStatus, NotificationMessagePage,
    NotificationRecipientOption, SystemNotificationRecord,
};
use serde_json::{json, Value};
use sqlx::{postgres::PgRow, Row};
use time::OffsetDateTime;
use uuid::Uuid;

fn instance(row: PgRow) -> Result<NotificationInstanceRecord> {
    Ok(NotificationInstanceRecord {
        id: row.get("id"),
        workspace_id: row.get("workspace_id"),
        type_code: row.get("type_code"),
        installation_id: row.get("installation_id"),
        display_name: row.get("display_name"),
        status: match row.get::<String, _>("status").as_str() {
            "enabled" => NotificationInstanceStatus::Enabled,
            "disabled" => NotificationInstanceStatus::Disabled,
            _ => return Err(Error::Conflict("notification_status").into()),
        },
        config_json: row.get("config_json"),
        configured_secret_fields: serde_json::from_value(row.get("configured_secret_fields"))?,
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}
fn message(row: PgRow) -> SystemNotificationRecord {
    SystemNotificationRecord {
        id: row.get("id"),
        workspace_id: row.get("workspace_id"),
        instance_id: row.get("instance_id"),
        recipient_user_id: row.get("recipient_user_id"),
        title: row.get("title"),
        content: row.get("content"),
        read_at: row.get("read_at"),
        created_at: row.get("created_at"),
    }
}
pub(crate) fn aad(workspace: Uuid, id: Uuid) -> Vec<u8> {
    format!("notification-instance:{workspace}:{id}").into_bytes()
}

#[async_trait]
impl NotificationRepository for PgControlPlaneStore {
    async fn bootstrap_system_notification_instances(&self) -> Result<()> {
        sqlx::query("INSERT INTO notification_instances (id,workspace_id,type_code,display_name,is_builtin) SELECT gen_random_uuid(),id,'system','System notification',true FROM workspaces ON CONFLICT (workspace_id) WHERE is_builtin DO NOTHING").execute(self.pool()).await?;
        Ok(())
    }
    async fn list_notification_instances(
        &self,
        workspace_id: Uuid,
    ) -> Result<Vec<NotificationInstanceRecord>> {
        sqlx::query(
            "SELECT * FROM notification_instances WHERE workspace_id=$1 ORDER BY created_at,id",
        )
        .bind(workspace_id)
        .fetch_all(self.pool())
        .await?
        .into_iter()
        .map(instance)
        .collect()
    }
    async fn get_notification_instance(
        &self,
        workspace_id: Uuid,
        id: Uuid,
    ) -> Result<Option<NotificationInstanceRecord>> {
        sqlx::query("SELECT * FROM notification_instances WHERE workspace_id=$1 AND id=$2")
            .bind(workspace_id)
            .bind(id)
            .fetch_optional(self.pool())
            .await?
            .map(instance)
            .transpose()
    }
    async fn save_notification_instance(
        &self,
        input: SaveNotificationInstanceInput,
    ) -> Result<NotificationInstanceRecord> {
        let r = input.record;
        let encrypted = encrypt_secret_json_with_aad(
            &input.secret_config_json,
            &input.master_key,
            &aad(r.workspace_id, r.id),
        )?;
        let row = if input.create {
            sqlx::query("INSERT INTO notification_instances (id,workspace_id,type_code,installation_id,display_name,status,config_json,configured_secret_fields,encrypted_secret_json) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING *")
                .bind(r.id).bind(r.workspace_id).bind(&r.type_code).bind(r.installation_id).bind(&r.display_name).bind(r.status.as_str()).bind(&r.config_json).bind(json!(r.configured_secret_fields)).bind(encrypted).fetch_one(self.pool()).await?
        } else {
            sqlx::query("UPDATE notification_instances SET display_name=$3,status=$4,config_json=$5,configured_secret_fields=$6,encrypted_secret_json=$7,updated_at=now() WHERE workspace_id=$1 AND id=$2 AND updated_at=$8 RETURNING *")
                .bind(r.workspace_id).bind(r.id).bind(&r.display_name).bind(r.status.as_str()).bind(&r.config_json).bind(json!(r.configured_secret_fields)).bind(encrypted).bind(r.updated_at).fetch_optional(self.pool()).await?.ok_or(Error::Conflict("notification_instance_changed"))?
        };
        instance(row)
    }
    async fn delete_notification_instance(&self, workspace_id: Uuid, id: Uuid) -> Result<bool> {
        Ok(
            sqlx::query("DELETE FROM notification_instances WHERE workspace_id=$1 AND id=$2")
                .bind(workspace_id)
                .bind(id)
                .execute(self.pool())
                .await?
                .rows_affected()
                > 0,
        )
    }
    async fn resolve_notification_secret_config(
        &self,
        workspace_id: Uuid,
        id: Uuid,
        master_key: &str,
    ) -> Result<Value> {
        let value:Option<Value>=sqlx::query_scalar("SELECT encrypted_secret_json FROM notification_instances WHERE workspace_id=$1 AND id=$2").bind(workspace_id).bind(id).fetch_optional(self.pool()).await?.ok_or(Error::NotFound("notification_instance"))?;
        match value {
            Some(value) => decrypt_secret_json_with_aad(&value, master_key, &aad(workspace_id, id)),
            None => Ok(json!({})),
        }
    }
    async fn notification_recipient_options(
        &self,
        workspace_id: Uuid,
    ) -> Result<Vec<NotificationRecipientOption>> {
        Ok(sqlx::query("SELECT u.id,coalesce(nullif(u.name,''),nullif(u.nickname,''),u.account) AS display_name FROM workspace_memberships wm JOIN users u ON u.id=wm.user_id WHERE wm.workspace_id=$1 AND u.status='active' ORDER BY display_name,u.id").bind(workspace_id).fetch_all(self.pool()).await?.into_iter().map(|r|NotificationRecipientOption{id:r.get("id"),display_name:r.get("display_name")}).collect())
    }
    async fn commit_system_notifications(
        &self,
        workspace_id: Uuid,
        instance_id: Uuid,
        recipients: &[Uuid],
        title: &str,
        content: &str,
    ) -> Result<Vec<SystemNotificationRecord>> {
        let mut tx = self.pool().begin().await?;
        let status:Option<String>=sqlx::query_scalar("SELECT status FROM notification_instances WHERE workspace_id=$1 AND id=$2 AND type_code='system' FOR SHARE").bind(workspace_id).bind(instance_id).fetch_optional(&mut *tx).await?;
        match status.as_deref() {
            Some("enabled") => {}
            Some(_) => return Err(Error::Conflict("instance_disabled").into()),
            None => return Err(Error::NotFound("notification_instance").into()),
        }
        let valid:Vec<Uuid>=sqlx::query_scalar("SELECT u.id FROM workspace_memberships wm JOIN users u ON u.id=wm.user_id WHERE wm.workspace_id=$1 AND u.id=ANY($2) AND u.status='active' FOR SHARE OF wm,u").bind(workspace_id).bind(recipients).fetch_all(&mut *tx).await?;
        if valid.len() != recipients.len() {
            return Err(Error::InvalidInput("notification_recipients").into());
        }
        let mut records = Vec::with_capacity(recipients.len());
        for user_id in recipients {
            records.push(message(sqlx::query("INSERT INTO system_notifications (id,workspace_id,instance_id,recipient_user_id,title,content) VALUES ($1,$2,$3,$4,$5,$6) RETURNING *").bind(Uuid::now_v7()).bind(workspace_id).bind(instance_id).bind(user_id).bind(title).bind(content).fetch_one(&mut *tx).await?));
        }
        tx.commit().await?;
        Ok(records)
    }
    async fn list_system_notifications(
        &self,
        workspace_id: Uuid,
        user_id: Uuid,
        input: NotificationMessageListInput,
    ) -> Result<NotificationMessagePage> {
        let size = input.page_size.unwrap_or(50);
        if size == 0 {
            return Err(Error::InvalidInput("page_size").into());
        }
        let cursor = input
            .cursor
            .map(|cursor| -> Result<(OffsetDateTime, Uuid)> {
                let (ts, id) = cursor
                    .split_once(':')
                    .ok_or(Error::InvalidInput("cursor"))?;
                Ok((
                    OffsetDateTime::from_unix_timestamp_nanos(
                        ts.parse().map_err(|_| Error::InvalidInput("cursor"))?,
                    )
                    .map_err(|_| Error::InvalidInput("cursor"))?,
                    Uuid::parse_str(id).map_err(|_| Error::InvalidInput("cursor"))?,
                ))
            })
            .transpose()?;
        let mut rows=sqlx::query("SELECT * FROM system_notifications WHERE workspace_id=$1 AND recipient_user_id=$2 AND (NOT $3 OR read_at IS NULL) AND ($4::timestamptz IS NULL OR (created_at,id)<($4,$5)) ORDER BY created_at DESC,id DESC LIMIT $6")
            .bind(workspace_id).bind(user_id).bind(input.unread_only).bind(cursor.map(|c|c.0)).bind(cursor.map(|c|c.1)).bind(i64::from(size)+1).fetch_all(self.pool()).await?.into_iter().map(message).collect::<Vec<_>>();
        let more = rows.len() > size as usize;
        rows.truncate(size as usize);
        let next_cursor = if more {
            rows.last()
                .map(|r| format!("{}:{}", r.created_at.unix_timestamp_nanos(), r.id))
        } else {
            None
        };
        Ok(NotificationMessagePage {
            items: rows,
            next_cursor,
            unread_count: self
                .system_notification_unread_count(workspace_id, user_id)
                .await?,
        })
    }
    async fn system_notification_unread_count(
        &self,
        workspace_id: Uuid,
        user_id: Uuid,
    ) -> Result<i64> {
        Ok(sqlx::query_scalar("SELECT count(*) FROM system_notifications WHERE workspace_id=$1 AND recipient_user_id=$2 AND read_at IS NULL").bind(workspace_id).bind(user_id).fetch_one(self.pool()).await?)
    }
    async fn mark_system_notification_read(
        &self,
        workspace_id: Uuid,
        user_id: Uuid,
        id: Uuid,
    ) -> Result<Option<SystemNotificationRecord>> {
        Ok(sqlx::query("UPDATE system_notifications SET read_at=coalesce(read_at,now()) WHERE workspace_id=$1 AND recipient_user_id=$2 AND id=$3 RETURNING *").bind(workspace_id).bind(user_id).bind(id).fetch_optional(self.pool()).await?.map(message))
    }
    async fn mark_all_system_notifications_read(
        &self,
        workspace_id: Uuid,
        user_id: Uuid,
    ) -> Result<u64> {
        Ok(sqlx::query("UPDATE system_notifications SET read_at=now() WHERE workspace_id=$1 AND recipient_user_id=$2 AND read_at IS NULL").bind(workspace_id).bind(user_id).execute(self.pool()).await?.rows_affected())
    }
}

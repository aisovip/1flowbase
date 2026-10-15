//! Bounded business turns from the persisted task/message projections.
use super::*;

const SESSION: &str = "t.application_id=a.application_id and t.scope_id=a.scope_id and t.source_kind=a.source_kind and ((a.source_kind='native' and ((a.log_conversation_id is not null and t.log_conversation_id=a.log_conversation_id) or t.id=a.id)) or (a.source_kind='imported' and a.log_conversation_id is not null and t.log_conversation_id=a.log_conversation_id and t.source_id=a.source_id and t.source_client=a.source_client and t.parent_task_run_id is null and t.parent_source_task_id is null))";

pub(in crate::orchestration_runtime_repository) async fn page(
    store: &PgControlPlaneStore,
    application_id: Uuid,
    record_id: Uuid,
    before: Option<Uuid>,
    after: Option<Uuid>,
    limit: i64,
) -> Result<ApplicationLogRecordConversationPage> {
    let membership = SESSION;
    if before.is_some() && after.is_some() {
        return Err(ControlPlaneError::InvalidInput("record_conversation_cursor").into());
    }
    let cursor = before.or(after);
    // Validate membership before using cursor time. A foreign cursor never alters the scope.
    let sql = format!(
        "select t.id from application_run_log_tasks a join application_run_log_tasks t on {membership} where a.application_id=$1 and a.id=$2 and ($3::uuid is null or t.id=$3) limit 1"
    );
    // SQL safety: Membership is static SQL; comparison and direction are closed literal selections, cursor values are bound.
    let valid: Option<Uuid> = sqlx::query_scalar(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(application_id)
        .bind(record_id)
        .bind(cursor)
        .fetch_optional(store.pool())
        .await?;
    if valid.is_none() {
        let exists: bool = sqlx::query_scalar("select exists(select 1 from application_run_log_tasks where application_id=$1 and id=$2)")
            .bind(application_id).bind(record_id).fetch_one(store.pool()).await?;
        if !exists {
            return Err(ControlPlaneError::NotFound("log_record").into());
        }
        return Err(ControlPlaneError::InvalidInput("record_conversation_cursor").into());
    }
    let comparison = if after.is_some() {
        ">"
    } else if before.is_some() {
        "<"
    } else {
        "<="
    };
    let direction = if after.is_some() { "asc" } else { "desc" };
    let sql = format!(
        "select t.id from application_run_log_tasks a join application_run_log_tasks t on {membership} left join application_run_log_tasks c on c.id=$3 where a.application_id=$1 and a.id=$2 and ($3::uuid is null or (t.started_at,t.id) {comparison} (c.started_at,c.id)) order by t.started_at {direction},t.id {direction} limit $4"
    );
    let mut ids: Vec<Uuid> = sqlx::query_scalar(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(application_id)
        .bind(record_id)
        .bind(cursor)
        .bind(limit.clamp(1, 100))
        .fetch_all(store.pool())
        .await?;
    if after.is_none() {
        ids.reverse();
    }
    let first = ids.first().copied().unwrap_or(cursor.unwrap_or(record_id));
    let last = ids.last().copied().unwrap_or(cursor.unwrap_or(record_id));
    let sql = format!(
        "select exists(select 1 from application_run_log_tasks t where {membership} and (t.started_at,t.id)<(f.started_at,f.id)) as has_before, exists(select 1 from application_run_log_tasks t where {membership} and (t.started_at,t.id)>(l.started_at,l.id)) as has_after from application_run_log_tasks a join application_run_log_tasks f on f.id=$3 join application_run_log_tasks l on l.id=$4 where a.application_id=$1 and a.id=$2"
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(application_id)
        .bind(record_id)
        .bind(first)
        .bind(last)
        .fetch_one(store.pool())
        .await?;
    let mut items = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(item) = super::overview(store, application_id, id).await? {
            items.push(item);
        }
    }
    Ok(ApplicationLogRecordConversationPage {
        page: ApplicationLogRecordConversationPageInfo {
            has_before: row.get("has_before"),
            has_after: row.get("has_after"),
            before_cursor: items.first().map(|v| v.record_id),
            after_cursor: items.last().map(|v| v.record_id),
            newest_cursor: items.last().map(|v| v.record_id),
        },
        items,
    })
}

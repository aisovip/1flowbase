//! Bounded business turns from the persisted task/message projections.
use super::*;

const SESSION: &str = "t.application_id=a.application_id and t.scope_id=a.scope_id and t.source_kind=a.source_kind and ((a.source_kind='native' and a.log_conversation_id is not null and t.log_conversation_id=a.log_conversation_id) or (a.source_kind<>'native' and a.source_id is not null and a.source_client is not null and nullif(a.source_session_id,'') is not null and t.source_id=a.source_id and t.source_client=a.source_client and t.source_session_id=a.source_session_id) or t.id=a.id) and (a.source_kind='native' or a.parent_task_run_id is not null or t.parent_task_run_id is null)";

// Direct children never broaden to a session; reject even corrupted cross-namespace links.
const CHILDREN: &str = "t.parent_task_run_id=a.id and t.id<>a.id and t.application_id=a.application_id and t.scope_id=a.scope_id and t.source_kind=a.source_kind and t.source_id is not distinct from a.source_id and t.source_client is not distinct from a.source_client and (a.source_kind='native' or (a.source_id is not null and a.source_client is not null))";

pub(in crate::orchestration_runtime_repository) async fn children_page(
    store: &PgControlPlaneStore,
    application_id: Uuid,
    record_id: Uuid,
    before: Option<Uuid>,
    after: Option<Uuid>,
    limit: i64,
) -> Result<ApplicationLogRecordConversationPage> {
    scoped_page(store, application_id, record_id, before, after, limit, true).await
}

pub(in crate::orchestration_runtime_repository) async fn page(
    store: &PgControlPlaneStore,
    application_id: Uuid,
    record_id: Uuid,
    before: Option<Uuid>,
    after: Option<Uuid>,
    limit: i64,
) -> Result<ApplicationLogRecordConversationPage> {
    scoped_page(
        store,
        application_id,
        record_id,
        before,
        after,
        limit,
        false,
    )
    .await
}

async fn scoped_page(
    store: &PgControlPlaneStore,
    application_id: Uuid,
    record_id: Uuid,
    before: Option<Uuid>,
    after: Option<Uuid>,
    limit: i64,
    children: bool,
) -> Result<ApplicationLogRecordConversationPage> {
    let membership = if children { CHILDREN } else { SESSION };
    if before.is_some() && after.is_some() {
        return Err(ControlPlaneError::InvalidInput("record_conversation_cursor").into());
    }
    let cursor = before
        .or(after)
        .or_else(|| (!children).then_some(record_id));
    // Validate membership before using cursor time. A foreign cursor never alters the scope.
    let sql = format!(
        "select t.id from application_run_log_tasks a join application_run_log_tasks t on {membership} where a.application_id=$1 and a.id=$2 and ($3::uuid is null or t.id=$3) limit 1"
    );
    let valid: Option<Uuid> = sqlx::query_scalar(&sql)
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
        if children && cursor.is_none() {
            return Ok(ApplicationLogRecordConversationPage {
                items: vec![],
                page: ApplicationLogRecordConversationPageInfo {
                    has_before: false,
                    has_after: false,
                    before_cursor: None,
                    after_cursor: None,
                    newest_cursor: None,
                },
            });
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
    let mut ids: Vec<Uuid> = sqlx::query_scalar(&sql)
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
    let row = sqlx::query(&sql)
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

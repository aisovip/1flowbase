//! Lazy task hierarchy: no transcript or raw observation bodies are selected.
use super::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

const EDGE: &str = "child.parent_task_run_id=t.id and child.id<>t.id and child.application_id=t.application_id and child.scope_id=t.scope_id and child.source_kind=t.source_kind and child.source_id is not distinct from t.source_id and child.source_client is not distinct from t.source_client and (t.source_kind='native' or (t.source_id is not null and t.source_client is not null))";
// Walk only the requested node's ancestor chain. Root child paging never
// enumerates unrelated descendants merely to establish membership.
const ANCESTORS: &str = "with recursive ancestors as (select t.id,t.parent_task_run_id,t.application_id,t.scope_id,t.source_kind,t.source_id,t.source_client,array[t.id] as path from application_run_log_tasks t where t.application_id=$1 and t.id=$3 union all select parent.id,parent.parent_task_run_id,parent.application_id,parent.scope_id,parent.source_kind,parent.source_id,parent.source_client,a.path||parent.id from ancestors a join application_run_log_tasks parent on parent.id=a.parent_task_run_id and parent.application_id=a.application_id and parent.scope_id=a.scope_id and parent.source_kind=a.source_kind and parent.source_id is not distinct from a.source_id and parent.source_client is not distinct from a.source_client and (a.source_kind='native' or (a.source_id is not null and a.source_client is not null)) where a.id<>$2 and not parent.id=any(a.path))";

fn projection() -> String {
    format!("select t.id,t.parent_task_run_id,t.title,t.status,t.started_at,t.finished_at,(extract(epoch from (t.finished_at-t.started_at))*1000)::bigint as duration_ms,t.source_client,t.source_session_id,t.source_task_id,jsonb_build_object('total_tokens',t.total_tokens,'input_tokens',t.input_tokens,'output_tokens',t.output_tokens,'input_cache_hit_tokens',t.input_cache_hit_tokens,'total_cost',t.total_cost::text) as metrics_payload,(select count(*) from application_run_log_tasks child where {EDGE}) as child_count,exists(select 1 from client_trajectory_steps s where s.record_id=t.id or s.flow_run_id=t.native_run_id or (t.source_kind='native' and s.flow_run_id=t.id)) as has_content from application_run_log_tasks t")
}
fn node(row: sqlx::postgres::PgRow, root: Uuid) -> Result<ApplicationLogRecordTraceNode> {
    let id: Uuid = row.try_get("id")?;
    let child_count: i64 = row.try_get("child_count")?;
    Ok(ApplicationLogRecordTraceNode {
        trace_node_id: id,
        record_id: id,
        parent_trace_node_id: if id == root {
            None
        } else {
            row.try_get("parent_task_run_id")?
        },
        node_kind: if id == root {
            "task_round"
        } else {
            "child_task"
        }
        .into(),
        node_alias: row.try_get("title")?,
        status: row.try_get("status")?,
        started_at: row
            .try_get::<time::OffsetDateTime, _>("started_at")?
            .format(&time::format_description::well_known::Rfc3339)?,
        finished_at: row
            .try_get::<Option<time::OffsetDateTime>, _>("finished_at")?
            .map(|v| v.format(&time::format_description::well_known::Rfc3339))
            .transpose()?,
        duration_ms: row.try_get("duration_ms")?,
        metrics_payload: row.try_get("metrics_payload")?,
        has_children: child_count > 0,
        child_count,
        has_content: row.try_get("has_content")?,
        source_client: row.try_get("source_client")?,
        source_session_id: row.try_get("source_session_id")?,
        source_task_id: row.try_get("source_task_id")?,
    })
}
pub(in crate::orchestration_runtime_repository) async fn tree(
    store: &PgControlPlaneStore,
    application_id: Uuid,
    record_id: Uuid,
) -> Result<ApplicationLogRecordTraceTree> {
    let sql = format!("{} where t.application_id=$1 and t.id=$2", projection());
    // SQL safety: EDGE/ANCESTORS/projection are static SQL fragments; trace and cursor identifiers remain bound.
    let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(application_id)
        .bind(record_id)
        .fetch_optional(store.pool())
        .await?
        .ok_or(ControlPlaneError::NotFound("log_record"))?;
    Ok(ApplicationLogRecordTraceTree {
        nodes: vec![node(row, record_id)?],
    })
}
pub(in crate::orchestration_runtime_repository) async fn children(
    store: &PgControlPlaneStore,
    application_id: Uuid,
    record_id: Uuid,
    trace_node_id: Uuid,
    cursor: Option<String>,
    page_size: i64,
) -> Result<ApplicationLogRecordTraceChildren> {
    let lookahead = log_query_page_lookahead(page_size)?;
    let sql = format!("{ANCESTORS} select exists(select 1 from ancestors where id=$2)");
    let valid: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(application_id)
        .bind(record_id)
        .bind(trace_node_id)
        .fetch_one(store.pool())
        .await?;
    if !valid {
        return Err(ControlPlaneError::NotFound("record_trace_node").into());
    }
    let position = cursor
        .as_deref()
        .map(|value| -> Result<Uuid> {
            let parsed: (u8, Uuid, Uuid, Uuid, Uuid) = URL_SAFE_NO_PAD
                .decode(value)
                .ok()
                .and_then(|v| serde_json::from_slice(&v).ok())
                .ok_or(ControlPlaneError::InvalidInput("record_trace_cursor"))?;
            if (parsed.0, parsed.1, parsed.2, parsed.3)
                != (1, application_id, record_id, trace_node_id)
            {
                return Err(ControlPlaneError::InvalidInput("record_trace_cursor").into());
            }
            Ok(parsed.4)
        })
        .transpose()?;
    if let Some(position) = position {
        let sql=format!("select exists(select 1 from application_run_log_tasks t join application_run_log_tasks child on {EDGE} where t.application_id=$1 and t.id=$2 and child.id=$3)");
        let valid: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(application_id)
            .bind(trace_node_id)
            .bind(position)
            .fetch_one(store.pool())
            .await?;
        if !valid {
            return Err(ControlPlaneError::InvalidInput("record_trace_cursor").into());
        }
    }
    let sql=format!("{} join application_run_log_tasks parent on t.parent_task_run_id=parent.id and t.application_id=parent.application_id and t.scope_id=parent.scope_id and t.source_kind=parent.source_kind and t.source_id is not distinct from parent.source_id and t.source_client is not distinct from parent.source_client where parent.application_id=$1 and parent.id=$2 and t.id<>parent.id and (parent.source_kind='native' or (parent.source_id is not null and parent.source_client is not null)) and ($3::uuid is null or t.id>$3) order by t.id limit $4",projection());
    let mut rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(application_id)
        .bind(trace_node_id)
        .bind(position)
        .bind(lookahead)
        .fetch_all(store.pool())
        .await?;
    let has_more = rows.len() > page_size as usize;
    rows.truncate(page_size as usize);
    let items = rows
        .into_iter()
        .map(|r| node(r, record_id))
        .collect::<Result<Vec<_>>>()?;
    let next_cursor = if has_more {
        items
            .last()
            .map(|n| {
                serde_json::to_vec(&(
                    1u8,
                    application_id,
                    record_id,
                    trace_node_id,
                    n.trace_node_id,
                ))
                .map(|v| URL_SAFE_NO_PAD.encode(v))
            })
            .transpose()?
    } else {
        None
    };
    Ok(ApplicationLogRecordTraceChildren {
        items,
        page_info: ApplicationLogRecordTracePageInfo {
            has_more,
            next_cursor,
            page_size,
        },
    })
}

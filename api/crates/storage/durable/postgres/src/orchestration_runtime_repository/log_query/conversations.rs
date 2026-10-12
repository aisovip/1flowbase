//! Conversation membership is matched on main task facts, then aggregated once.
use super::*;
use crate::runtime_record_repository::{append_filter_clause, to_runtime_model_metadata};
use sqlx::{Postgres, QueryBuilder};

pub(in crate::orchestration_runtime_repository) async fn conversations(
    store: &PgControlPlaneStore,
    scope_id: Uuid,
    application_ids: &[Uuid],
    query: &ApplicationLogConversationsQuery,
) -> Result<ApplicationLogConversationsPage> {
    validate_application_log_query_filter(&query.filter)?;
    let sort = match query.sort_field.as_str() {
        "started_at"
        | "finished_at"
        | "created_at"
        | "updated_at"
        | "total_tokens"
        | "input_tokens"
        | "output_tokens"
        | "input_cache_hit_tokens"
        | "total_cost" => format!("g.{}", query.sort_field),
        "duration_ms" => "extract(epoch from (g.finished_at-g.started_at))*1000".into(),
        _ => return Err(invalid("log_query_sort")),
    };
    log_query_page_lookahead(query.page_size)?;
    if query.page < 1 {
        return Err(invalid("log_query_page"));
    }
    let offset = (query.page - 1)
        .checked_mul(query.page_size)
        .ok_or_else(|| invalid("log_query_page"))?;
    let fields = application_log_query_fields();
    let mut metadata = to_runtime_model_metadata(
        control_plane_contracts::ports::ModelDefinitionRepository::list_model_definitions(
            store,
            Uuid::nil(),
        )
        .await?
        .into_iter()
        .find(|m| m.code == "application_run_log_tasks" && m.scope_id == domain::SYSTEM_SCOPE_ID)
        .ok_or_else(|| anyhow!("application_run_log_tasks metadata missing"))?,
    )?;
    for field in &fields {
        if field.value_type == LogQueryValueType::Uuid {
            if let Some(declared) = metadata.fields.iter_mut().find(|f| f.code == field.field) {
                declared.field_kind = domain::ModelFieldKind::ManyToOne;
            }
        }
    }
    let build = || -> Result<QueryBuilder<'static, Postgres>> {
        let mut sql = QueryBuilder::new(
            "with main as (select * from application_run_log_tasks where scope_id=",
        );
        sql.push_bind(scope_id).push(" and application_id=any(").push_bind(application_ids.to_vec())
            .push(") and log_conversation_id is not null and parent_task_run_id is null and (source_kind='native' or parent_source_task_id is null)), matched as (select distinct log_conversation_id from main where true");
        append_filter_clause(&mut sql, &metadata, &query.filter)
            .map_err(|_| invalid("log_query_filter"))?;
        sql.push("), latest as (select distinct on (m.log_conversation_id) m.* from main m join matched x using(log_conversation_id) order by m.log_conversation_id,m.started_at desc,m.id desc), grouped as (select l.id,l.application_id,l.source_kind,l.source_id,l.source_client,l.source_session_id,l.source_task_id,l.native_run_id,l.log_conversation_id,l.requested_model_id,l.reasoning_effort,l.status,l.outcome,l.title,min(t.started_at) as started_at,max(t.finished_at) as finished_at,min(t.created_at) as created_at,max(t.updated_at) as updated_at,sum(t.total_tokens)::bigint as total_tokens,sum(t.input_tokens)::bigint as input_tokens,sum(t.output_tokens)::bigint as output_tokens,sum(t.input_cache_hit_tokens)::bigint as input_cache_hit_tokens,sum(t.total_cost) as total_cost from latest l join application_run_log_tasks t on t.log_conversation_id=l.log_conversation_id and t.application_id=l.application_id and t.scope_id=l.scope_id and t.source_kind=l.source_kind and t.source_id is not distinct from l.source_id and t.source_client is not distinct from l.source_client group by l.id,l.application_id,l.source_kind,l.source_id,l.source_client,l.source_session_id,l.source_task_id,l.native_run_id,l.log_conversation_id,l.requested_model_id,l.reasoning_effort,l.status,l.outcome,l.title) ");
        Ok(sql)
    };
    let mut tx = store.pool().begin().await?;
    sqlx::query("set transaction isolation level repeatable read read only")
        .execute(&mut *tx)
        .await?;
    let mut count = build()?;
    count.push("select count(*) from grouped");
    let total: i64 = count.build_query_scalar().fetch_one(&mut *tx).await?;
    let mut sql = build()?;
    sql.push("select g.id,g.application_id,g.source_kind,g.source_id,g.source_client,g.source_session_id,g.source_task_id,g.native_run_id,g.log_conversation_id,g.requested_model_id,g.reasoning_effort,g.status,g.outcome,g.title,g.total_tokens,g.input_tokens,g.output_tokens,g.input_cache_hit_tokens,g.total_cost::text as total_cost,g.started_at,g.finished_at,g.created_at,g.updated_at from grouped g order by ")
        .push(sort).push(if query.descending {" desc"} else {" asc"})
        .push(" nulls last,g.log_conversation_id").push(if query.descending {" desc"} else {" asc"})
        .push(" limit ").push_bind(query.page_size).push(" offset ").push_bind(offset);
    let items = sql
        .build()
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(super::records::summary)
        .collect::<Result<Vec<_>>>()?;
    tx.commit().await?;
    Ok(ApplicationLogConversationsPage {
        items,
        total,
        page: query.page,
        page_size: query.page_size,
    })
}

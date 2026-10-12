//! Canonical imported conversation membership and bounded timestamp-keyset scans.
use super::*;

const MEMBERS: &str = "from application_conversations c join applications a on a.id=c.application_id and a.scope_id=c.scope_id join application_run_log_tasks t on t.log_conversation_id=c.id and t.application_id=c.application_id and t.scope_id=c.scope_id and t.source_kind=c.source_kind and t.source_id=c.source_id and t.source_client=c.source_client";
const BOUNDARY: &str = "c.application_id=$1 and c.id=$2 and c.source_kind='imported'";
// observed_at is the original persisted timestamp text. Removing the fraction
// before timestamptz conversion avoids PostgreSQL's microsecond rounding;
// numeric retains every original fractional digit and normalizes UTC offsets.
const EVENT_TIME: &str = "(extract(epoch from regexp_replace(s.observed_at,'[.]([0-9]+)','')::timestamptz) + coalesce(('0.' || substring(s.observed_at from '[.]([0-9]+)'))::numeric,0))";

#[derive(Clone, Serialize, Deserialize)]
struct Position {
    timestamp: String,
    step_id: Uuid,
    record_id: Uuid,
}
#[derive(Serialize, Deserialize)]
struct PagePosition {
    previous: bool,
    position: Position,
}
struct Namespace {
    scope_id: Uuid,
    source_id: String,
    source_client: String,
}
async fn namespace(
    store: &PgControlPlaneStore,
    app: Uuid,
    conversation: Uuid,
) -> Result<Namespace> {
    let row = sqlx::query("select c.scope_id,c.source_id,c.source_client from application_conversations c join applications a on a.id=c.application_id and a.scope_id=c.scope_id where c.application_id=$1 and c.id=$2 and c.source_kind='imported' and nullif(c.source_id,'') is not null and nullif(c.source_client,'') is not null")
        .bind(app).bind(conversation).fetch_optional(store.pool()).await?
        .ok_or(ControlPlaneError::NotFound("log_conversation"))?;
    Ok(Namespace {
        scope_id: row.try_get("scope_id")?,
        source_id: row.try_get("source_id")?,
        source_client: row.try_get("source_client")?,
    })
}

pub(in crate::orchestration_runtime_repository) async fn conversation_section(
    store: &PgControlPlaneStore,
    app: Uuid,
    conversation: Uuid,
    step: Uuid,
    section: &str,
    cursor: Option<i64>,
    limit: i64,
) -> Result<Option<ClientTrajectorySection>> {
    namespace(store, app, conversation).await?;
    let sql = format!("select t.id {MEMBERS} join client_trajectory_steps s on s.record_id=t.id where {BOUNDARY} and s.id=$3");
    let owner: Option<Uuid> = sqlx::query_scalar(&sql)
        .bind(app)
        .bind(conversation)
        .bind(step)
        .fetch_optional(store.pool())
        .await?;
    let Some(owner) = owner else {
        return Err(ControlPlaneError::NotFound("log_trajectory_step").into());
    };
    super::super::agent_logs::section(store, app, owner, step, section, cursor, limit).await
}

async fn semantic_matches(
    store: &PgControlPlaneStore,
    app: Uuid,
    item: &ConversationClientTrajectoryStep,
    keyword: Option<&str>,
    sections: &[String],
) -> Result<Vec<RecordTrajectoryMatch>> {
    let Some(keyword) = keyword else {
        return Ok(vec![]);
    };
    let mut matches = vec![];
    for section in sections {
        if !item.step.available_sections.contains(section) {
            continue;
        }
        let mut cursor = None;
        loop {
            let Some(part) = super::super::agent_logs::section(
                store,
                app,
                item.record_id,
                item.step.id,
                section,
                cursor,
                32,
            )
            .await?
            else {
                break;
            };
            if let Some(found) = part.items.iter().find_map(|value| {
                // Imported overview is a source fact containing a raw archive
                // sibling. Search only its semantic metadata, preserving the
                // full original value for the separately authorized reader.
                let snippet = if section == "overview" {
                    match value.value.as_object() {
                        Some(fields) => fields
                            .iter()
                            .filter(|(key, _)| key.as_str() != "raw")
                            .find_map(|(key, body)| {
                                log_query_snippet(key, keyword)
                                    .or_else(|| super::trajectory::semantic_snippet(body, keyword))
                            }),
                        None => super::trajectory::semantic_snippet(&value.value, keyword),
                    }
                } else {
                    super::trajectory::semantic_snippet(&value.value, keyword)
                };
                snippet.map(|snippet| RecordTrajectoryMatch {
                    step_id: item.step.id,
                    section: section.clone(),
                    sequence: value.sequence,
                    snippet,
                })
            }) {
                matches.push(found);
                break;
            }
            match part.next_cursor {
                Some(next) if cursor.is_none_or(|previous| next > previous) => cursor = Some(next),
                Some(_) => return Err(anyhow!("log_query.section_cursor_not_advancing")),
                None => break,
            }
        }
    }
    Ok(matches)
}

struct Hit {
    item: ConversationClientTrajectoryStep,
    position: Position,
    matches: Vec<RecordTrajectoryMatch>,
}
// Cursor direction and acquisition bounds form one scan state, independent of membership.
struct ScanWindow<'a> {
    position: Option<Position>,
    previous: bool,
    focus_time: Option<&'a str>,
    count: usize,
}

/// Only the source scan quantum is fixed. Result size and total search scope
/// have no arbitrary ceiling; semantic bodies are opened only when requested.
async fn scan(
    store: &PgControlPlaneStore,
    app: Uuid,
    conversation: Uuid,
    query: &ConversationClientTrajectoryQuery,
    sections: &[String],
    window: ScanWindow<'_>,
) -> Result<Vec<Hit>> {
    let ScanWindow {
        mut position,
        previous,
        focus_time,
        count,
    } = window;
    let fields = conversation_trajectory_query_fields();
    let comparison = if previous { "<" } else { ">" };
    let direction = if previous { "desc" } else { "asc" };
    let sql = format!("select runtime_original_json(s.metadata,s.raw_json_payloads,'metadata') as body,
        {EVENT_TIME}::text as event_time,s.id as step_id,t.id as record_id,t.log_conversation_id,
        t.parent_task_run_id,(t.parent_task_run_id is not null or t.parent_source_task_id is not null) as is_subagent,
        t.source_kind,t.source_id,t.source_client,t.source_session_id,t.source_task_id
        {MEMBERS} join client_trajectory_steps s on s.record_id=t.id
        where {BOUNDARY} and ($3::numeric is null or ({EVENT_TIME},s.id,t.id) {comparison} ($3::numeric,$4::uuid,$5::uuid))
        and ($6::numeric is null or {EVENT_TIME} >= $6::numeric)
        order by {EVENT_TIME} {direction},s.id {direction},t.id {direction} limit 100");
    let mut hits = Vec::new();
    loop {
        let rows = sqlx::query(&sql)
            .bind(app)
            .bind(conversation)
            .bind(position.as_ref().map(|p| p.timestamp.as_str()))
            .bind(position.as_ref().map(|p| p.step_id))
            .bind(position.as_ref().map(|p| p.record_id))
            .bind(focus_time)
            .fetch_all(store.pool())
            .await?;
        let exhausted = rows.len() < 100;
        for row in rows {
            let next = Position {
                timestamp: row.try_get("event_time")?,
                step_id: row.try_get("step_id")?,
                record_id: row.try_get("record_id")?,
            };
            let item = ConversationClientTrajectoryStep {
                step: serde_json::from_value(row.try_get("body")?)?,
                record_id: next.record_id,
                log_conversation_id: conversation,
                parent_task_run_id: row.try_get("parent_task_run_id")?,
                is_subagent: row.try_get("is_subagent")?,
                source_kind: row.try_get("source_kind")?,
                source_id: row.try_get("source_id")?,
                source_client: row.try_get("source_client")?,
                source_session_id: row.try_get("source_session_id")?,
                source_task_id: row.try_get("source_task_id")?,
            };
            position = Some(next.clone());
            if !log_query_filter_matches(&query.filter, &serde_json::to_value(&item)?, &fields) {
                continue;
            }
            let matches =
                semantic_matches(store, app, &item, query.keyword.as_deref(), sections).await?;
            if query.keyword.is_some() && matches.is_empty() {
                continue;
            }
            hits.push(Hit {
                item,
                position: next,
                matches,
            });
            if hits.len() == count {
                return Ok(hits);
            }
        }
        if exhausted {
            return Ok(hits);
        }
    }
}

pub(in crate::orchestration_runtime_repository) async fn conversation_trajectory(
    store: &PgControlPlaneStore,
    app: Uuid,
    conversation: Uuid,
    query: &ConversationClientTrajectoryQuery,
) -> Result<ConversationClientTrajectoryQueryPage> {
    let lookahead = usize::try_from(log_query_page_lookahead(query.limit)?)
        .map_err(|_| invalid("log_query_limit"))?;
    validate_log_query_filter(&query.filter, &conversation_trajectory_query_fields())?;
    let sections = log_query_search_sections(&RecordClientTrajectoryQuery {
        filter: query.filter.clone(),
        cursor: None,
        limit: query.limit,
        keyword: query.keyword.clone(),
        search_sections: query.search_sections.clone(),
    })?;
    let ns = namespace(store, app, conversation).await?;
    let identity = conversation_trajectory_query_fingerprint(
        app,
        ns.scope_id,
        conversation,
        &ns.source_id,
        &ns.source_client,
        query,
        &sections,
    );
    let page: Option<PagePosition> = decode(query.cursor.as_deref(), &identity)?;
    // Validate focus even when following a cursor: it remains part of query meaning.
    let focus_time = if let Some(focus) = query.focus_record_id {
        let sql = format!("select coalesce(min({EVENT_TIME}),extract(epoch from t.started_at))::text {MEMBERS} left join client_trajectory_steps s on s.record_id=t.id where {BOUNDARY} and t.id=$3 group by t.id");
        let time: Option<Option<String>> = sqlx::query_scalar(&sql)
            .bind(app)
            .bind(conversation)
            .bind(focus)
            .fetch_optional(store.pool())
            .await?;
        Some(
            time.ok_or_else(|| invalid("conversation_trajectory_focus"))?
                .ok_or_else(|| invalid("conversation_trajectory_focus_empty"))?,
        )
    } else {
        None
    };
    let focus_step = if let Some(step_id) = query.focus_step_id {
        let sql = format!("select {EVENT_TIME}::text event_time,s.id step_id,t.id record_id {MEMBERS} join client_trajectory_steps s on s.record_id=t.id where {BOUNDARY} and s.id=$3");
        let row = sqlx::query(&sql)
            .bind(app)
            .bind(conversation)
            .bind(step_id)
            .fetch_optional(store.pool())
            .await?
            .ok_or_else(|| invalid("conversation_trajectory_focus_step"))?;
        Some(Position {
            timestamp: row.try_get("event_time")?,
            step_id: row.try_get("step_id")?,
            record_id: row.try_get("record_id")?,
        })
    } else {
        None
    };
    if query.index_only {
        let overview = conversation_index(store, app, conversation, query, &sections).await?;
        return Ok(ConversationClientTrajectoryQueryPage {
            log_conversation_id: conversation,
            items: vec![],
            matches: vec![],
            previous_cursor: None,
            next_cursor: None,
            search_sections: vec![],
            integrity: "complete".into(),
            overview: Some(overview),
        });
    }
    if let Some(page) = &page {
        // A cursor position must identify an actual event in this exact namespace.
        // Never cast untrusted numeric cursor text: compare its canonical rendering.
        let sql = format!("select exists(select 1 {MEMBERS} join client_trajectory_steps s on s.record_id=t.id where {BOUNDARY} and s.id=$3 and t.id=$4 and {EVENT_TIME}::text=$5)");
        let valid: bool = sqlx::query_scalar(&sql)
            .bind(app)
            .bind(conversation)
            .bind(page.position.step_id)
            .bind(page.position.record_id)
            .bind(&page.position.timestamp)
            .fetch_one(store.pool())
            .await?;
        if !valid {
            return Err(invalid("log_query_cursor"));
        }
    }
    let initial_position = if page.is_none() {
        if let Some(target) = &focus_step {
            let sql=format!("select {EVENT_TIME}::text event_time,s.id step_id,t.id record_id {MEMBERS} join client_trajectory_steps s on s.record_id=t.id where {BOUNDARY} and ({EVENT_TIME},s.id,t.id)<($3::numeric,$4::uuid,$5::uuid) order by {EVENT_TIME} desc,s.id desc,t.id desc limit 1");
            sqlx::query(&sql)
                .bind(app)
                .bind(conversation)
                .bind(&target.timestamp)
                .bind(target.step_id)
                .bind(target.record_id)
                .fetch_optional(store.pool())
                .await?
                .map(|r| {
                    Ok::<_, anyhow::Error>(Position {
                        timestamp: r.try_get("event_time")?,
                        step_id: r.try_get("step_id")?,
                        record_id: r.try_get("record_id")?,
                    })
                })
                .transpose()?
        } else {
            None
        }
    } else {
        None
    };
    let initial_page = page.is_none();
    let previous = page.as_ref().is_some_and(|p| p.previous);
    let initial_focus = if page.is_none() && focus_step.is_none() {
        focus_time.as_deref()
    } else {
        None
    };
    let mut hits = scan(
        store,
        app,
        conversation,
        query,
        &sections,
        ScanWindow {
            position: page.map(|p| p.position).or(initial_position),
            previous,
            focus_time: initial_focus,
            count: lookahead,
        },
    )
    .await?;
    let mut reverse = previous;
    if hits.is_empty() {
        if let Some(timestamp) = initial_focus {
            // A focus with no matching newer events still exposes matching older
            // history. The synthetic bound is never emitted as a cursor.
            hits = scan(
                store,
                app,
                conversation,
                query,
                &sections,
                ScanWindow {
                    position: Some(Position {
                        timestamp: timestamp.into(),
                        step_id: Uuid::nil(),
                        record_id: Uuid::nil(),
                    }),
                    previous: true,
                    focus_time: None,
                    count: lookahead,
                },
            )
            .await?;
            reverse = true;
        }
    }
    if hits.is_empty() && initial_page {
        if let Some(target) = focus_step {
            hits = scan(
                store,
                app,
                conversation,
                query,
                &sections,
                ScanWindow {
                    position: Some(target),
                    previous: true,
                    focus_time: None,
                    count: lookahead,
                },
            )
            .await?;
            reverse = true;
        }
    }
    hits.truncate(lookahead - 1);
    if reverse {
        hits.reverse();
    }
    let mut previous_cursor = None;
    let mut next_cursor = None;
    if let (Some(first), Some(last)) = (hits.first(), hits.last()) {
        if !scan(
            store,
            app,
            conversation,
            query,
            &sections,
            ScanWindow {
                position: Some(first.position.clone()),
                previous: true,
                focus_time: None,
                count: 1,
            },
        )
        .await?
        .is_empty()
        {
            previous_cursor = Some(encode(
                PagePosition {
                    previous: true,
                    position: first.position.clone(),
                },
                &identity,
            )?);
        }
        if !scan(
            store,
            app,
            conversation,
            query,
            &sections,
            ScanWindow {
                position: Some(last.position.clone()),
                previous: false,
                focus_time: None,
                count: 1,
            },
        )
        .await?
        .is_empty()
        {
            next_cursor = Some(encode(
                PagePosition {
                    previous: false,
                    position: last.position.clone(),
                },
                &identity,
            )?);
        }
    }
    let mut items = vec![];
    let mut matches = vec![];
    for hit in hits {
        items.push(hit.item);
        matches.extend(hit.matches);
    }
    Ok(ConversationClientTrajectoryQueryPage {
        overview: None,
        log_conversation_id: conversation,
        items,
        matches,
        previous_cursor,
        next_cursor,
        search_sections: if query.keyword.is_some() {
            sections
        } else {
            vec![]
        },
        integrity: "complete".into(),
    })
}

async fn conversation_index(
    store: &PgControlPlaneStore,
    app: Uuid,
    conversation: Uuid,
    query: &ConversationClientTrajectoryQuery,
    sections: &[String],
) -> Result<
    control_plane_contracts::ports::TrajectoryOverview<
        control_plane_contracts::ports::ConversationTrajectoryIndexItem,
    >,
> {
    use control_plane_contracts::ports::{
        ClientTrajectoryIndexItem, ConversationTrajectoryIndexItem, TrajectoryOverview,
    };
    let mut items = Vec::new();
    let simple = matches!(&query.filter, domain::ResourceFilterExpr::All(filters) if filters.is_empty())
        && query.keyword.is_none();
    if simple {
        // Exact source order and identities only. No restored metadata/body projection.
        let sql=format!("select s.id,s.request_id,s.event_sequence,s.observed_at,s.metadata->>'category' category,s.metadata->>'protocol' protocol,s.flow_run_id,s.node_run_id,t.id record_id,(t.parent_task_run_id is not null or t.parent_source_task_id is not null) is_subagent {MEMBERS} join client_trajectory_steps s on s.record_id=t.id where {BOUNDARY} order by {EVENT_TIME},s.id,t.id");
        for r in sqlx::query(&sql)
            .bind(app)
            .bind(conversation)
            .fetch_all(store.pool())
            .await?
        {
            items.push(ConversationTrajectoryIndexItem {
                step: ClientTrajectoryIndexItem {
                    protocol: r.try_get("protocol")?,
                    id: r.try_get("id")?,
                    request_id: r.try_get("request_id")?,
                    sequence: r.try_get("event_sequence")?,
                    created_at: r.try_get("observed_at")?,
                    category: r.try_get("category")?,
                    flow_run_id: r.try_get("flow_run_id")?,
                    node_run_id: r.try_get("node_run_id")?,
                },
                record_id: r.try_get("record_id")?,
                log_conversation_id: conversation,
                is_subagent: r.try_get("is_subagent")?,
            });
        }
    } else {
        // Interactive predicates retain canonical lossless and semantic-search behavior.
        let mut position = None;
        loop {
            let hits = scan(
                store,
                app,
                conversation,
                query,
                sections,
                position.clone(),
                false,
                None,
                100,
            )
            .await?;
            let done = hits.len() < 100;
            for hit in hits {
                position = Some(hit.position);
                let s = hit.item;
                items.push(ConversationTrajectoryIndexItem {
                    step: ClientTrajectoryIndexItem {
                        protocol: s.step.protocol,
                        id: s.step.id,
                        request_id: s.step.request_id,
                        sequence: s.step.sequence,
                        created_at: s.step.created_at,
                        category: s.step.category,
                        flow_run_id: s.step.flow_run_id,
                        node_run_id: s.step.node_run_id,
                    },
                    record_id: s.record_id,
                    log_conversation_id: conversation,
                    is_subagent: s.is_subagent,
                });
            }
            if done {
                break;
            }
        }
    }
    let mut category_counts = std::collections::BTreeMap::new();
    for p in &items {
        *category_counts.entry(p.step.category.clone()).or_insert(0) += 1;
    }
    let time_start = items.first().map(|p| p.step.created_at.clone());
    let time_end = items.last().map(|p| p.step.created_at.clone());
    Ok(TrajectoryOverview {
        request_roots: None,
        total_count: items.len() as i64,
        category_counts,
        time_start,
        time_end,
        items,
    })
}

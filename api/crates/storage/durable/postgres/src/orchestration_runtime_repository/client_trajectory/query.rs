//! Run-scoped acquisition. Exact index reads identities only; summary search uses restored metadata.
use super::*;
use control_plane_contracts::ports::{
    ClientTrajectoryIndexItem, ClientTrajectoryReadQuery, TrajectoryOverview, TrajectorySelection,
    TrajectoryTargetNotFound,
};
use std::collections::{BTreeMap, HashSet};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

fn timestamp(value: Option<&str>) -> Result<Option<OffsetDateTime>> {
    value
        .map(|v| {
            OffsetDateTime::parse(v, &Rfc3339).map_err(|_| {
                control_plane_contracts::ControlPlaneContractError::InvalidInput("trajectory_time")
                    .into()
            })
        })
        .transpose()
}

impl PgControlPlaneStore {
    pub(in crate::orchestration_runtime_repository) async fn read_client_trajectory_query(
        &self,
        run: Uuid,
        node: Option<Uuid>,
        query: ClientTrajectoryReadQuery,
    ) -> Result<ClientTrajectoryPage> {
        // Viewport windows with no interactive predicates stay bounded database reads.
        if !query.index_only
            && query.category.is_none()
            && query.keyword.as_deref().is_none_or(|s| s.trim().is_empty())
            && query.from.is_none()
            && query.to.is_none()
        {
            let previous = query.cursor.is_some_and(|n| n < 0);
            let cursor = query
                .cursor
                .map(|n| {
                    n.checked_abs().ok_or_else(|| {
                        control_plane_contracts::ControlPlaneContractError::InvalidInput(
                            "trajectory_cursor",
                        )
                    })
                })
                .transpose()?;
            let mut page = self
                .read_client_trajectory_window(
                    run,
                    node,
                    cursor,
                    query.limit,
                    TrajectorySelection {
                        request_id: query.request_id,
                        target_id: query.focus_step_id,
                    },
                    None,
                    previous,
                )
                .await?;
            if let (Some(first), Some(last)) = (page.items.first(), page.items.last()) {
                let sql="select exists(select 1 from client_trajectory_steps s where s.flow_run_id=$1 and ($2::uuid is null or s.node_run_id=$2 or exists(select 1 from client_trajectory_node_links l where l.request_id=s.request_id and l.node_run_id=$2)) and ($3::uuid is null or s.request_id=$3) and s.event_sequence<$4) older,exists(select 1 from client_trajectory_steps s where s.flow_run_id=$1 and ($2::uuid is null or s.node_run_id=$2 or exists(select 1 from client_trajectory_node_links l where l.request_id=s.request_id and l.node_run_id=$2)) and ($3::uuid is null or s.request_id=$3) and s.event_sequence>$5) newer";
                let boundaries = sqlx::query(sql)
                    .bind(run)
                    .bind(node)
                    .bind(query.request_id)
                    .bind(first.sequence)
                    .bind(last.sequence)
                    .fetch_one(self.pool())
                    .await?;
                page.previous_cursor = if boundaries.get::<bool, _>("older") {
                    Some(-first.sequence)
                } else {
                    None
                };
                page.next_cursor = if boundaries.get::<bool, _>("newer") {
                    Some(last.sequence)
                } else {
                    None
                };
            }
            return Ok(page);
        }
        let from = timestamp(query.from.as_deref())?;
        let to = timestamp(query.to.as_deref())?;
        if from.zip(to).is_some_and(|(a, b)| a > b) {
            return Err(
                control_plane_contracts::ControlPlaneContractError::InvalidInput(
                    "trajectory_time_range",
                )
                .into(),
            );
        }
        // Category is a protocol classification, observed_at is persisted original time.
        // Do not restore the storage body or sections to build the complete index.
        let rows = sqlx::query("select s.id,s.request_id,s.flow_run_id,s.node_run_id,s.event_sequence,coalesce(s.metadata->>'created_at',s.metadata#>>'{fact,step,created_at}',s.observed_at) observed_at,coalesce(s.metadata->>'category',s.metadata#>>'{fact,step,category}') category,coalesce(s.metadata->>'protocol',s.metadata#>>'{fact,step,protocol}','responses') protocol,case when coalesce(s.metadata->>'category',s.metadata#>>'{fact,step,category}')='request' then client_trajectory_step_storage_body(s.id,s.flow_run_id) end request_metadata,s.semantic_metadata_restored from client_trajectory_steps s where s.flow_run_id=$1 and ($2::uuid is null or s.node_run_id=$2 or exists(select 1 from client_trajectory_node_links l where l.request_id=s.request_id and l.node_run_id=$2)) and ($3::uuid is null or s.request_id=$3) order by s.event_sequence")
            .bind(run).bind(node).bind(query.request_id).fetch_all(self.pool()).await?;
        let mut index: Vec<ClientTrajectoryIndexItem> = rows
            .into_iter()
            .map(|r| {
                let preview = match r.get::<Option<Value>, _>("request_metadata") {
                    Some(body) => {
                        semantic::step_metadata(body, r.get("semantic_metadata_restored"))?
                            ["preview"]
                            .as_str()
                            .map(str::to_owned)
                    }
                    None => None,
                };
                Ok::<_, anyhow::Error>(ClientTrajectoryIndexItem {
                    protocol: r.get("protocol"),
                    preview,
                    id: r.get("id"),
                    request_id: r.get("request_id"),
                    flow_run_id: r.get("flow_run_id"),
                    node_run_id: r.get("node_run_id"),
                    sequence: r.get("event_sequence"),
                    created_at: r.get("observed_at"),
                    category: r.get("category"),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let request_roots = index
            .iter()
            .filter(|p| p.category == "request")
            .cloned()
            .collect::<Vec<_>>();
        let time_start = index
            .iter()
            .min_by_key(|p| timestamp(Some(&p.created_at)).ok().flatten())
            .map(|p| p.created_at.clone());
        let time_end = index
            .iter()
            .max_by_key(|p| timestamp(Some(&p.created_at)).ok().flatten())
            .map(|p| p.created_at.clone());
        // Focus is validated against the complete membership before interactive filtering.
        let focus = query
            .focus_step_id
            .map(|id| {
                index
                    .iter()
                    .find(|p| p.id == id)
                    .map(|p| p.sequence)
                    .ok_or(TrajectoryTargetNotFound)
            })
            .transpose()?;
        index.retain(|p| {
            timestamp(Some(&p.created_at))
                .ok()
                .flatten()
                .is_some_and(|time| from.is_none_or(|f| time >= f) && to.is_none_or(|t| time <= t))
        });
        let keyword = query.keyword.as_deref().unwrap_or("").trim().to_lowercase();
        if !keyword.is_empty() {
            // Preserve the existing six-field summary contains search, including restored NUL.
            let mut found = HashSet::new();
            let mut cursor = None;
            loop {
                let page = self
                    .read_client_trajectory_selected_page(
                        run,
                        node,
                        cursor,
                        100,
                        TrajectorySelection {
                            request_id: query.request_id,
                            target_id: None,
                        },
                    )
                    .await?;
                for step in page.items {
                    let text = [
                        Some(step.name.as_str()),
                        step.namespace.as_deref(),
                        Some(step.preview.as_str()),
                        step.call_id.as_deref(),
                        step.parameters_preview.as_deref(),
                        step.result_preview.as_deref(),
                    ]
                    .map(|v| v.unwrap_or(""))
                    .join(" ")
                    .to_lowercase();
                    if text.contains(&keyword) {
                        found.insert(step.id);
                    }
                }
                match page.next_cursor {
                    Some(next) if cursor.is_none_or(|old| next > old) => cursor = Some(next),
                    None => break,
                    _ => return Err(anyhow!("client trajectory cursor not advancing")),
                }
            }
            index.retain(|p| found.contains(&p.id));
        }
        let integrity: String = sqlx::query_scalar("select case when count(*)=0 then 'not_recorded' when bool_or(status not in ('pending','complete') or dropped_count>0 or persist_failed_count>0) then 'incomplete' when bool_or(status='pending') then 'pending' else 'complete' end from client_trajectory_captures c where flow_run_id=$1 and ($2::uuid is null or node_run_id=$2 or exists(select 1 from client_trajectory_node_links l where l.request_id=c.request_id and l.node_run_id=$2)) and ($3::uuid is null or c.request_id=$3)")
            .bind(run).bind(node).bind(query.request_id).fetch_one(self.pool()).await?;
        let mut counts = BTreeMap::new();
        for point in &index {
            *counts.entry(point.category.clone()).or_insert(0) += 1;
        }
        index.retain(|p| query.category.as_ref().is_none_or(|c| &p.category == c));
        if query.index_only {
            return Ok(ClientTrajectoryPage {
                items: vec![],
                previous_cursor: None,
                next_cursor: None,
                integrity,
                overview: Some(TrajectoryOverview {
                    request_roots: Some(
                        request_roots
                            .into_iter()
                            .filter(|root| index.iter().any(|p| p.request_id == root.request_id))
                            .collect(),
                    ),
                    total_count: index.len() as i64,
                    category_counts: counts,
                    time_start,
                    time_end,
                    items: index,
                }),
            });
        }
        let limit = query.limit.clamp(1, 100) as usize;
        // Negative cursors encode the previous exclusive sequence boundary.
        let previous = query.cursor.is_some_and(|n| n < 0);
        let boundary = query
            .cursor
            .map(|n| {
                n.checked_abs()
                    .ok_or_else(|| anyhow!("invalid trajectory cursor"))
            })
            .transpose()?;
        let mut candidates: Vec<_> = index
            .iter()
            .filter(|p| {
                boundary.is_none_or(|b| {
                    if previous {
                        p.sequence < b
                    } else {
                        p.sequence > b
                    }
                }) && (query.cursor.is_some() || focus.is_none_or(|f| p.sequence >= f))
            })
            .collect();
        if previous {
            candidates.reverse();
        }
        candidates.truncate(limit);
        if previous {
            candidates.reverse();
        }
        let ids: Vec<_> = candidates.iter().map(|p| p.id).collect();
        let first = candidates.first().map(|p| p.sequence);
        let last = candidates.last().map(|p| p.sequence);
        let previous_cursor = first
            .filter(|first| index.iter().any(|p| p.sequence < *first))
            .map(|n| -n);
        let next_cursor = last.filter(|last| index.iter().any(|p| p.sequence > *last));
        let items = if ids.is_empty() {
            vec![]
        } else {
            self.read_client_trajectory_window(
                run,
                node,
                first.map(|n| n - 1),
                limit as i64,
                TrajectorySelection {
                    request_id: query.request_id,
                    target_id: None,
                },
                Some(&ids),
            )
            .await?
            .items
        };
        Ok(ClientTrajectoryPage {
            items,
            previous_cursor,
            next_cursor,
            integrity,
            overview: None,
        })
    }
}

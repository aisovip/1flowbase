//! Resolve declared imported parents under the ingest source advisory lock.
use super::*;
use std::collections::HashMap;

pub(super) async fn resolve(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    application_id: Uuid,
    scope_id: Uuid,
    source_id: &str,
    source_client: &str,
) -> Result<()> {
    let rows: Vec<(Uuid, String, Option<String>)> = sqlx::query_as(
        "select id,source_task_id,parent_source_task_id from application_run_log_tasks where application_id=$1 and scope_id=$2 and source_kind='imported' and source_id=$3 and source_client=$4",
    ).bind(application_id).bind(scope_id).bind(source_id).bind(source_client)
        .fetch_all(&mut **tx).await?;
    // None marks an ambiguous identifier across sessions, including self.
    let mut identifiers: HashMap<&str, Option<usize>> = HashMap::new();
    for (index, (_, task, _)) in rows.iter().enumerate() {
        identifiers
            .entry(task)
            .and_modify(|value| *value = None)
            .or_insert(Some(index));
    }
    let mut edges: Vec<Option<usize>> = rows
        .iter()
        .enumerate()
        .map(|(index, (_, _, parent))| {
            parent
                .as_deref()
                .and_then(|key| identifiers.get(key).copied().flatten())
                .filter(|parent| *parent != index)
        })
        .collect();
    // Iterative three-color traversal of a functional graph: O(V+E), no
    // recursion or depth limit. Clear every edge belonging to a cycle.
    let mut colors = vec![0u8; rows.len()];
    let mut positions = vec![0usize; rows.len()];
    for start in 0..rows.len() {
        if colors[start] != 0 {
            continue;
        }
        let mut path = Vec::new();
        let mut current = Some(start);
        while let Some(node) = current {
            if colors[node] != 0 {
                if colors[node] == 1 {
                    for &cyclic in &path[positions[node]..] {
                        edges[cyclic] = None;
                    }
                }
                break;
            }
            colors[node] = 1;
            positions[node] = path.len();
            path.push(node);
            current = edges[node];
        }
        for node in path {
            colors[node] = 2;
        }
    }
    let ids: Vec<Uuid> = rows.iter().map(|row| row.0).collect();
    let parents: Vec<Option<Uuid>> = edges
        .iter()
        .map(|edge| edge.map(|index| rows[index].0))
        .collect();
    // Recompute the whole namespace on changed batches: late parents bind,
    // late ambiguity unbinds. Duplicate-only batches never reach this writer.
    sqlx::query("update application_run_log_tasks t set parent_task_run_id=r.parent from unnest($1::uuid[],$2::uuid[]) as r(id,parent) where t.id=r.id and t.parent_task_run_id is distinct from r.parent")
        .bind(ids).bind(parents).execute(&mut **tx).await?;
    sqlx::query("select refresh_imported_conversation_ownership($1,$2,$3,$4)")
        .bind(application_id)
        .bind(scope_id)
        .bind(source_id)
        .bind(source_client)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

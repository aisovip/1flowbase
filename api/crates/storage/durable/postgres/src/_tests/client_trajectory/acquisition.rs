use super::*;
#[tokio::test]
async fn client_acquisition_exact_index_request_scope_late_search_and_bidirectional_focus() {
    let (pool, flow) = super::super::provider_protocol_capsule_store_tests::seeded_flow_run().await;
    let store = PgControlPlaneStore::new(pool);
    let request = Uuid::now_v7();
    let foreign_request = Uuid::now_v7();
    begin(&store, flow, None, request).await;
    begin(&store, flow, None, foreign_request).await;
    let mut ids = vec![];
    for index in 0..105 {
        let id = Uuid::now_v7();
        ids.push(id);
        let mut item = step(
            flow,
            None,
            request,
            id,
            "emitted",
            if index == 0 { "request" } else { "tool_call" },
        );
        if index == 104 {
            item.name = "LaTe 漢字\0needle".into();
            item.created_at = "2026-09-22T00:30:00Z".into();
        }
        append(
            &store,
            flow,
            None,
            request,
            ClientTrajectoryFact::Step {
                step: Box::new(item),
            },
        )
        .await;
    }
    let other = Uuid::now_v7();
    append(
        &store,
        flow,
        None,
        foreign_request,
        ClientTrajectoryFact::Step {
            step: Box::new(step(
                flow,
                None,
                foreign_request,
                other,
                "emitted",
                "tool_call",
            )),
        },
    )
    .await;
    let read = |index_only| ClientTrajectoryReadQuery {
        request_id: Some(request),
        limit: 1,
        index_only,
        ..Default::default()
    };
    let index_page = store
        .client_trajectory_query_page(flow, None, read(true))
        .await
        .unwrap();
    assert!(index_page.items.is_empty());
    let overview = index_page.overview.unwrap();
    assert_eq!(overview.total_count, 105);
    assert_eq!(overview.items.iter().map(|p| p.id).collect::<Vec<_>>(), ids);
    assert_eq!(overview.category_counts.get("request"), Some(&1));
    let wire = serde_json::to_value(&overview).unwrap();
    for point in wire["items"].as_array().unwrap() {
        assert!(point.get("available_sections").is_none());
        assert!(point.get("parameters_preview").is_none());
        assert!(point.get("preview").is_none());
    }
    let focused = store
        .client_trajectory_query_page(
            flow,
            None,
            ClientTrajectoryReadQuery {
                focus_step_id: ids.last().copied(),
                ..read(false)
            },
        )
        .await
        .unwrap();
    assert_eq!(focused.items[0].id, ids[104]);
    assert!(focused.previous_cursor.is_some());
    let older = store
        .client_trajectory_query_page(
            flow,
            None,
            ClientTrajectoryReadQuery {
                cursor: focused.previous_cursor,
                ..read(false)
            },
        )
        .await
        .unwrap();
    assert_eq!(older.items[0].id, ids[103]);
    let newer = store
        .client_trajectory_query_page(
            flow,
            None,
            ClientTrajectoryReadQuery {
                cursor: older.next_cursor,
                ..read(false)
            },
        )
        .await
        .unwrap();
    assert_eq!(newer.items[0].id, ids[104]);
    let searched = store
        .client_trajectory_query_page(
            flow,
            None,
            ClientTrajectoryReadQuery {
                keyword: Some("late 漢字\0NEEDLE".into()),
                ..read(true)
            },
        )
        .await
        .unwrap();
    assert_eq!(searched.overview.unwrap().items[0].id, ids[104]);
    assert!(store
        .client_trajectory_query_page(
            flow,
            None,
            ClientTrajectoryReadQuery {
                focus_step_id: Some(other),
                ..read(false)
            }
        )
        .await
        .is_err());
    let category = store
        .client_trajectory_query_page(
            flow,
            None,
            ClientTrajectoryReadQuery {
                category: Some("tool_call".into()),
                ..read(true)
            },
        )
        .await
        .unwrap()
        .overview
        .unwrap();
    assert_eq!(category.request_roots.as_ref().unwrap()[0].id, ids[0]);
    assert_ne!(category.request_roots.as_ref().unwrap()[0].id, request);
    assert_eq!(category.items.len(), 104);
    assert_eq!(category.category_counts.get("request"), Some(&1));
    assert_eq!(category.total_count, 104);
    let timed = store
        .client_trajectory_query_page(
            flow,
            None,
            ClientTrajectoryReadQuery {
                from: Some("2026-09-22T00:30:00Z".into()),
                ..read(true)
            },
        )
        .await
        .unwrap()
        .overview
        .unwrap();
    assert_eq!(timed.total_count, 1);
    assert_eq!(timed.items[0].id, ids[104]);
    assert_eq!(
        timed.time_start.as_deref(),
        Some(AT),
        "time brush retains the original scope domain"
    );
    assert_eq!(timed.time_end.as_deref(), Some("2026-09-22T00:30:00Z"));
    assert_eq!(timed.category_counts.get("tool_call"), Some(&1));
    assert!(timed.category_counts.get("request").is_none());
}

#[tokio::test]
async fn client_thin_index_never_opens_large_request_body_or_preview() {
    let (pool, flow) = super::super::provider_protocol_capsule_store_tests::seeded_flow_run().await;
    let store = PgControlPlaneStore::new(pool.clone());
    let request = Uuid::now_v7();
    let root = Uuid::now_v7();
    begin(&store, flow, None, request).await;
    let preview = "large-request-content\0".repeat(50_000);
    let mut item = step(flow, None, request, root, "submitted", "request");
    item.preview = preview.clone();
    append(
        &store,
        flow,
        None,
        request,
        ClientTrajectoryFact::Step {
            step: Box::new(item),
        },
    )
    .await;
    let originals: Value =
        sqlx::query_scalar("select raw_json_payloads from client_trajectory_steps where id=$1")
            .bind(root)
            .fetch_one(&pool)
            .await
            .unwrap();
    // A controlled invalid body would make any storage-body projection fail.
    // Identity/category/protocol remain valid and authorized in the index.
    sqlx::query("update client_trajectory_steps set raw_json_payloads=jsonb_set(raw_json_payloads,'{metadata}',to_jsonb('invalid request body'::text)) where id=$1").bind(root).execute(&pool).await.unwrap();
    let query = |index_only| ClientTrajectoryReadQuery {
        index_only,
        request_id: Some(request),
        limit: 1,
        ..Default::default()
    };
    let indexed = store
        .client_trajectory_query_page(flow, None, query(true))
        .await
        .unwrap();
    let overview = indexed.overview.unwrap();
    assert_eq!(overview.items[0].id, root);
    assert_eq!(overview.request_roots.as_ref().unwrap()[0].id, root);
    let wire = serde_json::to_value(&overview).unwrap();
    assert!(
        wire.to_string().len() < 4096,
        "overview must not transport the large request content"
    );
    for key in ["items", "request_roots"] {
        assert!(wire[key][0].get("preview").is_none());
        assert!(wire[key][0].get("body").is_none());
        assert!(wire[key][0].get("available_sections").is_none());
    }
    assert!(
        store
            .client_trajectory_query_page(flow, None, query(false))
            .await
            .is_err(),
        "the negative body fixture must reject a real viewport row read"
    );
    sqlx::query("update client_trajectory_steps set raw_json_payloads=$2 where id=$1")
        .bind(root)
        .bind(originals)
        .execute(&pool)
        .await
        .unwrap();
    let visible = store
        .client_trajectory_query_page(flow, None, query(false))
        .await
        .unwrap();
    assert_eq!(
        visible.items[0].preview, preview,
        "normal viewport rows keep the original lossless reader"
    );
}

use super::super::{
    stream,
    stream_ticket::{self, Ticket},
};
use serde_json::json;
fn ticket() -> Ticket {
    Ticket {
        session: "exact-session".into(),
        actor: uuid::Uuid::now_v7(),
        origin: "https://console.example".into(),
        installation: uuid::Uuid::now_v7(),
        generation: "g1".into(),
        epoch: 0,
        operation: "fixture.terminal".into(),
        payload: json!({"path":{},"body":{"host_id":"h"}}),
    }
}
#[tokio::test]
async fn stream_ticket_atomic_replay_origin_exact_session_and_generation_binding() {
    let cache = storage_ephemeral::MokaCacheStore::new("stream-test", 100);
    let ticket = ticket();
    let token = stream_ticket::store(&cache, &ticket).await.unwrap();
    assert_eq!(token.len(), 64);
    for (session, origin, generation, operation) in [
        (
            "another-session",
            ticket.origin.as_str(),
            "g1",
            ticket.operation.as_str(),
        ),
        (
            ticket.session.as_str(),
            "https://attacker.example",
            "g1",
            ticket.operation.as_str(),
        ),
        (
            ticket.session.as_str(),
            ticket.origin.as_str(),
            "g2",
            ticket.operation.as_str(),
        ),
        (
            ticket.session.as_str(),
            ticket.origin.as_str(),
            "g1",
            "fixture.other",
        ),
    ] {
        assert!(stream_ticket::claim(
            &cache,
            &token,
            session,
            ticket.actor,
            origin,
            ticket.installation,
            generation,
            operation
        )
        .await
        .is_err());
    }
    let a = stream_ticket::claim(
        &cache,
        &token,
        &ticket.session,
        ticket.actor,
        &ticket.origin,
        ticket.installation,
        &ticket.generation,
        &ticket.operation,
    );
    let b = stream_ticket::claim(
        &cache,
        &token,
        &ticket.session,
        ticket.actor,
        &ticket.origin,
        ticket.installation,
        &ticket.generation,
        &ticket.operation,
    );
    let (a, b) = tokio::join!(a, b);
    assert_ne!(a.is_ok(), b.is_ok());
    assert!(stream_ticket::claim(
        &cache,
        &token,
        &ticket.session,
        ticket.actor,
        &ticket.origin,
        ticket.installation,
        &ticket.generation,
        &ticket.operation
    )
    .await
    .is_err());
}
#[tokio::test]
async fn mutation_and_hot_generation_revoke_preissued_ticket() {
    let cache = storage_ephemeral::MokaCacheStore::new("stream-test", 100);
    let mut ticket = ticket();
    ticket.epoch = stream::epoch(ticket.installation);
    let token = stream_ticket::store(&cache, &ticket).await.unwrap();
    stream::revoke_installation(ticket.installation);
    assert!(stream_ticket::claim(
        &cache,
        &token,
        &ticket.session,
        ticket.actor,
        &ticket.origin,
        ticket.installation,
        &ticket.generation,
        &ticket.operation
    )
    .await
    .is_err());
    ticket.epoch = stream::epoch(ticket.installation);
    let token = stream_ticket::store(&cache, &ticket).await.unwrap();
    stream::revoke_all();
    assert!(stream_ticket::claim(
        &cache,
        &token,
        &ticket.session,
        ticket.actor,
        &ticket.origin,
        ticket.installation,
        &ticket.generation,
        &ticket.operation
    )
    .await
    .is_err());
}
#[test]
fn browser_origin_must_match_host_and_cannot_be_null_or_opaque() {
    let headers = |origin: &str| {
        let mut h = axum::http::HeaderMap::new();
        h.insert("host", "console.example".parse().unwrap());
        h.insert("origin", origin.parse().unwrap());
        h
    };
    assert_eq!(
        stream_ticket::origin(&headers("https://console.example")).unwrap(),
        "https://console.example"
    );
    for origin in [
        "null",
        "https://attacker.example",
        "https://console.example/path",
        "https://user@console.example",
    ] {
        assert!(stream_ticket::origin(&headers(origin)).is_err());
    }
}

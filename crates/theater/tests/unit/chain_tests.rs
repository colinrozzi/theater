use theater::chain::{ChainEvent, StateChain};
use theater::events::wasm::WasmEventData;
use theater::events::{ChainEventData, ChainEventPayload};
use theater::id::TheaterId;
use tokio::sync::mpsc;

fn create_test_event_data(event_type: &str, _data: &[u8]) -> ChainEventData {
    ChainEventData {
        event_type: event_type.to_string(),
        data: ChainEventPayload::Wasm(WasmEventData::WasmCall {
            function_name: "test-function".to_string(),
            params: theater::Value::Tuple(vec![]),
        }),
    }
}

#[tokio::test]
async fn test_chain_event_creation() {
    let actor_id = TheaterId::generate();
    let mut chain = StateChain::new(actor_id);

    let event_data = create_test_event_data("test-event", b"test data");
    let event = chain.add_typed_event(event_data).await.unwrap();

    assert_eq!(event.event_type, "test-event");
    assert!(event.parent_hash.is_none());
    assert!(!event.hash.is_empty());
    assert_eq!(chain.head_hash(), Some(event.hash.as_slice()));
}

/// The runtime no longer retains events, so chain integrity is verified by a
/// subscriber that collects events as they're emitted. This test plays that
/// subscriber role: register an mpsc subscriber, drive the chain, then assert
/// that each event's `parent_hash` matches its predecessor's `hash` and that
/// `head_hash` tracks the last emitted event.
#[tokio::test]
async fn test_chain_integrity_via_subscriber() {
    let actor_id = TheaterId::generate();
    let mut chain = StateChain::new(actor_id);

    let (sub_tx, mut sub_rx) = mpsc::channel(16);
    chain.add_subscriber(sub_tx);

    for i in 0..5 {
        let data = format!("event data {}", i);
        let event_data = create_test_event_data(&format!("event-{}", i), data.as_bytes());
        chain.add_typed_event(event_data).await.unwrap();
    }

    let mut collected: Vec<ChainEvent> = Vec::new();
    while let Ok((_id, event)) = sub_rx.try_recv() {
        collected.push(event);
    }
    assert_eq!(collected.len(), 5, "subscriber should see all 5 events");

    assert!(
        collected[0].parent_hash.is_none(),
        "first event has no parent"
    );
    for i in 1..collected.len() {
        assert_eq!(
            collected[i].parent_hash.as_ref().unwrap(),
            &collected[i - 1].hash,
            "event {} should link to event {}",
            i,
            i - 1
        );
    }

    assert_eq!(
        chain.head_hash(),
        Some(collected.last().unwrap().hash.as_slice()),
        "head_hash should match the last emitted event"
    );
}

/// Subscribers that attach mid-flight see only events emitted from the moment
/// of subscription forward. The chain does not backfill — that's the contract
/// subscribers must understand.
#[tokio::test]
async fn test_subscriber_attaches_after_first_event_sees_only_subsequent() {
    let actor_id = TheaterId::generate();
    let mut chain = StateChain::new(actor_id);

    chain
        .add_typed_event(create_test_event_data("event-0", b"a"))
        .await
        .unwrap();

    let (sub_tx, mut sub_rx) = mpsc::channel(16);
    chain.add_subscriber(sub_tx);

    chain
        .add_typed_event(create_test_event_data("event-1", b"b"))
        .await
        .unwrap();
    chain
        .add_typed_event(create_test_event_data("event-2", b"c"))
        .await
        .unwrap();

    let mut collected: Vec<ChainEvent> = Vec::new();
    while let Ok((_id, event)) = sub_rx.try_recv() {
        collected.push(event);
    }

    assert_eq!(
        collected.len(),
        2,
        "tail-only: pre-subscribe event excluded"
    );
    assert_eq!(collected[0].event_type, "event-1");
    assert_eq!(collected[1].event_type, "event-2");
}

/// `remove_subscriber` identifies the subscriber by mpsc channel identity
/// (Sender::same_channel), so any clone of the original Sender matches.
/// After removal, the subscriber stops receiving events; another subscriber
/// registered alongside still gets them.
#[tokio::test]
async fn test_remove_subscriber_stops_delivery_to_matched_channel() {
    let actor_id = TheaterId::generate();
    let mut chain = StateChain::new(actor_id);

    let (target_tx, mut target_rx) = mpsc::channel(16);
    let (other_tx, mut other_rx) = mpsc::channel(16);
    chain.add_subscriber(target_tx.clone());
    chain.add_subscriber(other_tx);

    chain
        .add_typed_event(create_test_event_data("pre-remove", b"a"))
        .await
        .unwrap();

    // Remove via a *clone* of the original sender. Channel-identity match
    // means any clone routing to the same receiver is sufficient.
    let removed = chain.remove_subscriber(&target_tx.clone());
    assert!(removed, "matching subscriber should be removed");

    chain
        .add_typed_event(create_test_event_data("post-remove", b"b"))
        .await
        .unwrap();

    let mut target_collected: Vec<ChainEvent> = Vec::new();
    while let Ok((_id, event)) = target_rx.try_recv() {
        target_collected.push(event);
    }
    let mut other_collected: Vec<ChainEvent> = Vec::new();
    while let Ok((_id, event)) = other_rx.try_recv() {
        other_collected.push(event);
    }

    assert_eq!(
        target_collected.len(),
        1,
        "target sees only the pre-remove event"
    );
    assert_eq!(target_collected[0].event_type, "pre-remove");
    assert_eq!(
        other_collected.len(),
        2,
        "unrelated subscriber still sees both"
    );
}

/// Removing a subscriber that was never registered returns `false` and
/// leaves existing subscribers intact — the host's idempotent
/// `unsubscribe-from-child` relies on this.
#[tokio::test]
async fn test_remove_subscriber_unknown_is_noop() {
    let actor_id = TheaterId::generate();
    let mut chain = StateChain::new(actor_id);

    let (kept_tx, mut kept_rx) = mpsc::channel(16);
    chain.add_subscriber(kept_tx);

    let (stranger_tx, _stranger_rx) = mpsc::channel::<(TheaterId, ChainEvent)>(16);
    let removed = chain.remove_subscriber(&stranger_tx);
    assert!(!removed, "no matching subscriber to remove");

    chain
        .add_typed_event(create_test_event_data("event-0", b"a"))
        .await
        .unwrap();

    let mut collected = 0;
    while kept_rx.try_recv().is_ok() {
        collected += 1;
    }
    assert_eq!(collected, 1, "kept subscriber still receives events");
}

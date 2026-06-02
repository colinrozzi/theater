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
    let (tx, _rx) = mpsc::channel(10);
    let actor_id = TheaterId::generate();
    let mut chain = StateChain::new(actor_id, tx);

    let event_data = create_test_event_data("test-event", b"test data");
    let event = chain.add_typed_event(event_data).unwrap();

    assert_eq!(event.event_type, "test-event");
    assert!(event.parent_hash.is_none());
    assert!(!event.hash.is_empty());
    assert_eq!(chain.head_hash(), Some(event.hash.as_slice()));
}

/// The runtime no longer retains events, so chain integrity is verified by a
/// subscriber that collects events as they're emitted. This test plays that
/// subscriber role: it grabs a `subscribe()` receiver, drives the chain, then
/// asserts that each event's `parent_hash` matches its predecessor's `hash`
/// and that `head_hash` tracks the last emitted event.
#[tokio::test]
async fn test_chain_integrity_via_subscriber() {
    let (tx, _rx) = mpsc::channel(10);
    let actor_id = TheaterId::generate();
    let mut chain = StateChain::new(actor_id, tx);
    let mut events_rx = chain.subscribe();

    for i in 0..5 {
        let data = format!("event data {}", i);
        let event_data = create_test_event_data(&format!("event-{}", i), data.as_bytes());
        chain.add_typed_event(event_data).unwrap();
    }

    let mut collected: Vec<ChainEvent> = Vec::new();
    while let Ok(event) = events_rx.try_recv() {
        collected.push(event);
    }
    assert_eq!(collected.len(), 5, "subscriber should see all 5 events");

    assert!(collected[0].parent_hash.is_none(), "first event has no parent");
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

/// New subscribers attach mid-flight see only events emitted from the moment
/// of subscription forward. The runtime does not backfill — that's the
/// contract subscribers must understand.
#[tokio::test]
async fn test_subscriber_attaches_after_first_event_sees_only_subsequent() {
    let (tx, _rx) = mpsc::channel(10);
    let actor_id = TheaterId::generate();
    let mut chain = StateChain::new(actor_id, tx);

    chain
        .add_typed_event(create_test_event_data("event-0", b"a"))
        .unwrap();

    let mut events_rx = chain.subscribe();

    chain
        .add_typed_event(create_test_event_data("event-1", b"b"))
        .unwrap();
    chain
        .add_typed_event(create_test_event_data("event-2", b"c"))
        .unwrap();

    let mut collected: Vec<ChainEvent> = Vec::new();
    while let Ok(event) = events_rx.try_recv() {
        collected.push(event);
    }

    assert_eq!(collected.len(), 2, "tail-only: pre-subscribe event excluded");
    assert_eq!(collected[0].event_type, "event-1");
    assert_eq!(collected[1].event_type, "event-2");
}

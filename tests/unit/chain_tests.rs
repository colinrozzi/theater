use anyhow::Result;
use chrono::Utc;
use theater::chain::{ChainEvent, StateChain};
use theater::events::{ChainEventData, EventData};
use theater::events::message::MessageEventData;
use theater::id::TheaterId;
use theater::messages::TheaterCommand;
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::tempdir;
use tokio::sync::mpsc;

fn create_test_event_data(event_type: &str, data: &[u8]) -> ChainEventData {
    ChainEventData {
        event_type: event_type.to_string(),
        data: EventData::Message(MessageEventData::HandleMessageCall {
            sender: "test-sender".to_string(),
            message_type: "test-message".to_string(),
            size: data.len(),
        }),
        timestamp: Utc::now().timestamp_millis() as u64,
        description: Some(format!("Test event: {}", event_type)),
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
    assert!(event.parent_hash.is_none()); // First event has no parent
    assert!(!event.hash.is_empty());
}

#[tokio::test]
async fn test_chain_integrity() {
    let (tx, _rx) = mpsc::channel(10);
    let actor_id = TheaterId::generate();
    let mut chain = StateChain::new(actor_id, tx);
    
    // Add multiple events to build a chain
    for i in 0..5 {
        let data = format!("event data {}", i);
        let event_data = create_test_event_data(&format!("event-{}", i), data.as_bytes());
        chain.add_typed_event(event_data).unwrap();
    }
    
    // Verify chain integrity
    assert!(chain.verify());
    assert_eq!(chain.get_events().len(), 5);
    
    // Check parent hash links
    let events = chain.get_events();
    for i in 1..events.len() {
        assert_eq!(events[i].parent_hash.as_ref().unwrap(), &events[i-1].hash);
    }
}

#[tokio::test]
async fn test_save_and_load_chain() {
    let (tx, _rx) = mpsc::channel(10);
    let actor_id = TheaterId::generate();
    let mut chain = StateChain::new(actor_id.clone(), tx);
    
    // Add events
    for i in 0..3 {
        let data = format!("event data {}", i);
        let event_data = create_test_event_data(&format!("event-{}", i), data.as_bytes());
        chain.add_typed_event(event_data).unwrap();
    }
    
    // Save to temp file
    let temp_dir = tempdir().unwrap();
    let file_path = temp_dir.path().join("chain_test.json");
    chain.save_to_file(&file_path).unwrap();
    
    // Check file exists with content
    assert!(file_path.exists());
    let content = std::fs::read_to_string(&file_path).unwrap();
    assert!(content.contains("event-0"));
    assert!(content.contains("event-1"));
    assert!(content.contains("event-2"));
}
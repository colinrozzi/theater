use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use theater::chain::{ChainEvent, StateChain};
use theater::id::TheaterId;
use theater::messages::{ActorMessage, TheaterCommand};
use tokio::sync::mpsc;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct TestMessage {
    value: String,
}

#[tokio::test]
async fn test_actor_message_creation() {
    let sender = TheaterId::generate();
    let recipient = TheaterId::generate();
    
    let test_msg = TestMessage {
        value: "test message content".to_string(),
    };
    
    let serialized = serde_json::to_vec(&test_msg).unwrap();
    
    let message = ActorMessage {
        sender: sender.clone(),
        recipient: recipient.clone(),
        payload: serialized,
    };
    
    assert_eq!(message.sender, sender);
    assert_eq!(message.recipient, recipient);
    
    // Deserialize payload and check
    let deserialized: TestMessage = serde_json::from_slice(&message.payload).unwrap();
    assert_eq!(deserialized, test_msg);
}

#[tokio::test]
async fn test_theater_command_stop_actor() {
    let actor_id = TheaterId::generate();
    
    let command = TheaterCommand::StopActor {
        actor_id: actor_id.clone()
    };
    
    match command {
        TheaterCommand::StopActor { actor_id: id } => {
            assert_eq!(id, actor_id);
        }
        _ => panic!("Wrong command type"),
    }
    
    // Test serialization
    let serialized = serde_json::to_string(&command).unwrap();
    let deserialized: TheaterCommand = serde_json::from_str(&serialized).unwrap();
    
    match deserialized {
        TheaterCommand::StopActor { actor_id: id } => {
            assert_eq!(id, actor_id);
        }
        _ => panic!("Wrong command type after deserialization"),
    }
}

#[tokio::test]
async fn test_theater_command_new_actor() {
    let actor_id = TheaterId::generate();
    let parent_id = TheaterId::generate();
    
    let command = TheaterCommand::NewActor {
        actor_id: actor_id.clone(),
        parent_id: Some(parent_id.clone()),
    };
    
    match command {
        TheaterCommand::NewActor { actor_id: id, parent_id: parent } => {
            assert_eq!(id, actor_id);
            assert_eq!(parent, Some(parent_id));
        }
        _ => panic!("Wrong command type"),
    }
    
    // Test serialization
    let serialized = serde_json::to_string(&command).unwrap();
    let deserialized: TheaterCommand = serde_json::from_str(&serialized).unwrap();
    
    match deserialized {
        TheaterCommand::NewActor { actor_id: id, parent_id: parent } => {
            assert_eq!(id, actor_id);
            assert_eq!(parent, Some(parent_id));
        }
        _ => panic!("Wrong command type after deserialization"),
    }
}

#[tokio::test]
async fn test_theater_command_new_event() {
    let actor_id = TheaterId::generate();
    let event = ChainEvent {
        hash: vec![1, 2, 3],
        parent_hash: None,
        event_type: "test-event".to_string(),
        data: vec![4, 5, 6],
        timestamp: Utc::now().timestamp_millis() as u64,
        description: Some("Test event description".to_string()),
    };
    
    let command = TheaterCommand::NewEvent {
        actor_id: actor_id.clone(),
        event: event.clone(),
    };
    
    match command {
        TheaterCommand::NewEvent { actor_id: id, event: e } => {
            assert_eq!(id, actor_id);
            assert_eq!(e.hash, event.hash);
            assert_eq!(e.event_type, event.event_type);
            assert_eq!(e.data, event.data);
            assert_eq!(e.timestamp, event.timestamp);
            assert_eq!(e.description, event.description);
        }
        _ => panic!("Wrong command type"),
    }
    
    // Test serialization
    let serialized = serde_json::to_string(&command).unwrap();
    let deserialized: TheaterCommand = serde_json::from_str(&serialized).unwrap();
    
    match deserialized {
        TheaterCommand::NewEvent { actor_id: id, event: e } => {
            assert_eq!(id, actor_id);
            assert_eq!(e.hash, event.hash);
            assert_eq!(e.event_type, event.event_type);
            assert_eq!(e.data, event.data);
            assert_eq!(e.timestamp, event.timestamp);
            assert_eq!(e.description, event.description);
        }
        _ => panic!("Wrong command type after deserialization"),
    }
}

#[tokio::test]
async fn test_command_channel() {
    let (tx, mut rx) = mpsc::channel::<TheaterCommand>(10);
    
    let actor_id = TheaterId::generate();
    let command = TheaterCommand::StopActor {
        actor_id: actor_id.clone()
    };
    
    // Send command
    tx.send(command).await.unwrap();
    
    // Receive and verify
    let received = rx.recv().await.unwrap();
    match received {
        TheaterCommand::StopActor { actor_id: id } => {
            assert_eq!(id, actor_id);
        }
        _ => panic!("Wrong command received"),
    }
}
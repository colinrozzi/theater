use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use theater::chain::{ChainEvent, StateChain};
use theater::id::TheaterId;
use theater::messages::{ActorMessage, ActorSend, TheaterCommand};
use tokio::sync::{mpsc, oneshot};

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
    
    let message = ActorMessage::Send(ActorSend {
        data: serialized,
    });
    
    // Check message type
    match message {
        ActorMessage::Send(send) => {
            // Deserialize payload and check
            let deserialized: TestMessage = serde_json::from_slice(&send.data).unwrap();
            assert_eq!(deserialized, test_msg);
        },
        _ => panic!("Wrong message type"),
    }
}

#[tokio::test]
async fn test_theater_command_stop_actor() {
    let actor_id = TheaterId::generate();
    
    let (tx, _rx) = oneshot::channel();
    let command = TheaterCommand::StopActor {
        actor_id: actor_id.clone(),
        response_tx: tx
    };
    
    match command {
        TheaterCommand::StopActor { actor_id: id, response_tx: _ } => {
            assert_eq!(id, actor_id);
        }
        _ => panic!("Wrong command type"),
    }
    
    // TheaterCommand doesn't implement Serialize/Deserialize
    // Just verify we can extract the command data correctly
    assert_eq!(command.to_log(), format!("StopActor: {:?}", actor_id));
}

#[tokio::test]
async fn test_theater_command_spawn_actor() {
    let manifest_path = "test_manifest.toml".to_string();
    let parent_id = TheaterId::generate();
    let (tx, _rx) = oneshot::channel();
    
    let command = TheaterCommand::SpawnActor {
        manifest_path: manifest_path.clone(),
        init_bytes: None,
        response_tx: tx,
        parent_id: Some(parent_id.clone()),
    };
    
    match command {
        TheaterCommand::SpawnActor { manifest_path: path, parent_id: parent, .. } => {
            assert_eq!(path, manifest_path);
            assert_eq!(parent, Some(parent_id));
        }
        _ => panic!("Wrong command type"),
    }
    
    // Verify logging output
    assert_eq!(command.to_log(), format!("SpawnActor: {}", manifest_path));
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
    
    // Verify logging output
    assert_eq!(command.to_log(), format!("NewEvent: {:?}", actor_id));
}

#[tokio::test]
async fn test_command_channel() {
    let (tx, mut rx) = mpsc::channel::<TheaterCommand>(10);
    
    let actor_id = TheaterId::generate();
    let (resp_tx, _resp_rx) = oneshot::channel();
    let command = TheaterCommand::StopActor {
        actor_id: actor_id.clone(),
        response_tx: resp_tx
    };
    
    // Send command
    tx.send(command).await.unwrap();
    
    // Receive and verify
    let received = rx.recv().await.unwrap();
    match received {
        TheaterCommand::StopActor { actor_id: id, response_tx: _ } => {
            assert_eq!(id, actor_id);
        }
        _ => panic!("Wrong command received"),
    }
}
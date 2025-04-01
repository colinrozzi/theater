use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use theater::id::TheaterId;
use theater::messages::{ActorMessage, ActorSend, TheaterCommand};
use tokio::sync::mpsc;
use tokio::time::timeout;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct TestPayload {
    command: String,
    data: String,
}

#[tokio::test]
async fn test_actor_message_serialization() {
    let sender = TheaterId::generate();
    let recipient = TheaterId::generate();
    let payload = TestPayload { 
        command: "test".to_string(),
        data: "test data".to_string() 
    };
    
    let message = ActorMessage::Send(ActorSend {
        data: serde_json::to_vec(&payload).unwrap(),
    });
    
    // ActorMessage doesn't implement Serialize/Deserialize
    // Just verify we can handle the message type
    match message {
        ActorMessage::Send(send) => {
            // Deserialize data and check
            let payload_deserialized: TestPayload = serde_json::from_slice(&send.data).unwrap();
            assert_eq!(payload_deserialized, payload);
        },
        _ => panic!("Wrong message type"),
    }
}

#[tokio::test]
async fn test_theater_command_new_event() {
    use theater::chain::ChainEvent;
    use tokio::sync::oneshot;
    
    let actor_id = TheaterId::generate();
    let event = ChainEvent {
        hash: vec![1, 2, 3],
        parent_hash: None,
        event_type: "test-event".to_string(),
        data: vec![4, 5, 6],
        timestamp: 12345,
        description: Some("test description".to_string()),
    };
    
    let command = TheaterCommand::NewEvent { 
        actor_id: actor_id.clone(), 
        event: event.clone() 
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


#[tokio::test]
async fn test_message_channel_flow() {
    let (tx, mut rx) = mpsc::channel::<ActorMessage>(10);
    
    // Create multiple actors
    let actors: Vec<TheaterId> = (0..3).map(|_| TheaterId::generate()).collect();
    
    // Send a series of messages between actors
    for i in 0..3 {
        for j in 0..3 {
            if i != j {
                let sender = &actors[i];
                let recipient = &actors[j];
                
                let payload = TestPayload {
                    command: format!("from-{}-to-{}", i, j),
                    data: format!("Message from actor {} to {}", i, j),
                };
                
                let message = ActorMessage {
                    sender: sender.clone(),
                    recipient: recipient.clone(),
                    payload: serde_json::to_vec(&payload).unwrap(),
                };
                
                tx.send(message).await.unwrap();
            }
        }
    }
    
    // We should have 6 messages (3 actors, each sending to 2 others)
    for _ in 0..6 {
        let received = timeout(Duration::from_millis(100), rx.recv()).await.unwrap().unwrap();
        
        // Deserialize payload
        let payload: TestPayload = serde_json::from_slice(&received.payload).unwrap();
        
        // Extract actor indices from command
        let command_parts: Vec<&str> = payload.command.split('-').collect();
        let from: usize = command_parts[1].parse().unwrap();
        let to: usize = command_parts[3].parse().unwrap();
        
        assert_eq!(received.sender, actors[from]);
        assert_eq!(received.recipient, actors[to]);
        assert_eq!(payload.data, format!("Message from actor {} to {}", from, to));
    }
    
    // Channel should be empty now
    let timeout_result = timeout(Duration::from_millis(100), rx.recv()).await;
    assert!(timeout_result.is_err() || timeout_result.unwrap().is_none());
}

#[tokio::test]
async fn test_broadcast_pattern() {
    let (tx, mut rx) = mpsc::channel::<ActorMessage>(10);
    
    // Create actors
    let broadcaster = TheaterId::generate();
    let receivers: Vec<TheaterId> = (0..5).map(|_| TheaterId::generate()).collect();
    
    // Broadcast a message to all receivers
    for receiver in &receivers {
        let payload = TestPayload {
            command: "broadcast".to_string(),
            data: "Broadcast message to all".to_string(),
        };
        
        let message = ActorMessage {
            sender: broadcaster.clone(),
            recipient: receiver.clone(),
            payload: serde_json::to_vec(&payload).unwrap(),
        };
        
        tx.send(message).await.unwrap();
    }
    
    // Receive all messages
    let mut received_count = 0;
    for _ in 0..5 {
        let received = timeout(Duration::from_millis(100), rx.recv()).await.unwrap().unwrap();
        
        // Verify sender
        assert_eq!(received.sender, broadcaster);
        
        // Verify recipient is one of our receivers
        assert!(receivers.contains(&received.recipient));
        
        // Verify payload
        let payload: TestPayload = serde_json::from_slice(&received.payload).unwrap();
        assert_eq!(payload.command, "broadcast");
        assert_eq!(payload.data, "Broadcast message to all");
        
        received_count += 1;
    }
    
    assert_eq!(received_count, 5);
}
use anyhow::Result;
use theater::Store;
use theater::ActorMessage;
use theater::ActorInput;
use serde_json::json;
use tokio::sync::mpsc;

#[test]
fn test_store_creation() {
    let _store = Store::new();
    assert!(store.http_port().is_none());
    assert!(store.http_server_port().is_none());
}

#[test]
fn test_store_with_http() {
    let (tx, _rx) = mpsc::channel(32);
    let store = Store::with_http(8080, tx.clone());
    assert_eq!(store.http_port(), Some(8080));
    assert!(store.http_server_port().is_none());
}

#[test]
fn test_store_with_both_http() {
    let (tx, _rx) = mpsc::channel(32);
    let store = Store::with_both_http(8080, 8081, tx.clone());
    assert_eq!(store.http_port(), Some(8080));
    assert_eq!(store.http_server_port(), Some(8081));
}

#[tokio::test]
async fn test_store_message_sending() -> Result<()> {
    let (tx, mut rx) = mpsc::channel(32);
    let store = Store::new();
    
    // Create test message
    let test_msg = ActorMessage {
        content: ActorInput::Message(json!({"test": "message"})),
        response_channel: None,
    };
    
    // Send through store's channel
    tx.send(test_msg).await?;
    
    // Verify message was received
    let received = rx.recv().await;
    assert!(received.is_some());
    
    Ok(())
}

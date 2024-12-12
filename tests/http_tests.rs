use anyhow::Result;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::time::Duration;
use theater::{ActorInput, ActorMessage, HostHandler};
use theater::http::{HttpHandler, HttpHost};
use theater::http_server::HttpServerHandler;
use tokio::sync::{mpsc, oneshot};

#[tokio::test]
async fn test_http_handler_creation() -> Result<()> {
    let config = json!({
        "port": 8080
    });
    
    let port = config["port"].as_u64().unwrap() as u16;
    let handler = HttpHandler::new(port);
    assert_eq!(handler.name(), "http");
    
    Ok(())
}

#[tokio::test]
async fn test_http_host_message_sending() -> Result<()> {
    let (tx, _rx) = mpsc::channel(32);
    let host = HttpHost::new(tx);
    
    // Test sending to invalid endpoint (should fail gracefully)
    let result = host.send_message(
        "http://localhost:1234".to_string(), 
        json!({"test": "message"})
    ).await;
    
    assert!(result.is_err());
    Ok(())
}

#[tokio::test]
async fn test_http_server_handler_creation() -> Result<()> {
    let config = json!({
        "port": 8081
    });
    
    let port = config["port"].as_u64().unwrap() as u16;
    let handler = HttpServerHandler::new(port);
    assert_eq!(handler.name(), "Http-server");
    
    Ok(())
}

#[tokio::test]
async fn test_http_server_request_handling() -> Result<()> {
    // Set up channels
    let (tx, mut rx) = mpsc::channel(32);
    let (response_tx, response_rx) = oneshot::channel();
    
    // Create test request
    let msg = ActorMessage {
        content: ActorInput::HttpRequest {
            method: "GET".to_string(),
            uri: "/test".to_string(),
            headers: vec![],
            body: None,
        },
        response_channel: Some(response_tx),
    };
    
    // Send message
    tx.send(msg).await?;
    
    // Verify message was received
    let received = rx.recv().await;
    assert!(received.is_some());
    
    // Clean up
    drop(rx);
    drop(response_rx);
    
    Ok(())
}

#[tokio::test]
async fn test_http_handler_start_stop() -> Result<()> {
    let port = json!({"port": 8082})["port"].as_u64().unwrap() as u16;
    let handler = HttpHandler::new(port);
    let (tx, _rx) = mpsc::channel(32);
    
    // Start handler in spawned task
    tokio::spawn(async move {
        let start_handle = handler.start(tx);
        let _ = start_handle.await;
    });
    
    // Give it time to start
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    Ok(())
}

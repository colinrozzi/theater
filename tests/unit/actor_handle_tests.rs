use serde::{Deserialize, Serialize};
use theater::actor_executor::{ActorError, ActorOperation};
use theater::actor_handle::ActorHandle;
use theater::chain::ChainEvent;
use theater::metrics::{ActorMetrics, OperationMetrics, ResourceMetrics};
use tokio::sync::mpsc;
use tokio::time::Duration;
use std::time::SystemTime;

// Helper for creating a test actor handle with controlled channel
async fn setup_test_handle() -> (ActorHandle, mpsc::Receiver<ActorOperation>) {
    let (tx, rx) = mpsc::channel(10);
    let handle = ActorHandle::new(tx);
    (handle, rx)
}

#[derive(Debug, Serialize, Deserialize)]
struct TestParams {
    value: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct TestResult {
    result: String,
}

#[tokio::test]
async fn test_get_state() {
    let (handle, mut rx) = setup_test_handle().await;
    
    // Spawn a task to respond to the GetState operation
    let test_data = vec![1, 2, 3, 4];
    let test_data_clone = test_data.clone();
    tokio::spawn(async move {
        if let Some(ActorOperation::GetState { response_tx }) = rx.recv().await {
            let _ = response_tx.send(Ok(Some(test_data_clone)));
        }
    });
    
    // Call the get_state method and verify result
    let result = handle.get_state().await.unwrap();
    assert_eq!(result, Some(test_data));
}

#[tokio::test]
async fn test_get_metrics() {
    let (handle, mut rx) = setup_test_handle().await;
    
    // Create test metrics
    let test_metrics = ActorMetrics {
        operation_metrics: OperationMetrics {
            total_operations: 10,
            failed_operations: 0,
            total_processing_time: Duration::from_millis(1000),
            max_processing_time: Duration::from_millis(200),
            min_processing_time: Some(Duration::from_millis(10)),
        },
        resource_metrics: ResourceMetrics {
            memory_usage: 1024,
            operation_queue_size: 5,
            peak_memory_usage: 2048,
            peak_queue_size: 10,
        },
        last_update: Some(SystemTime::now()),
        uptime_secs: 60,
        start_time: SystemTime::now(),
    };
    
    let test_metrics_clone = test_metrics.clone();
    tokio::spawn(async move {
        if let Some(ActorOperation::GetMetrics { response_tx }) = rx.recv().await {
            let _ = response_tx.send(Ok(test_metrics_clone));
        }
    });
    
    // Call get_metrics and verify
    let result = handle.get_metrics().await.unwrap();
    // Verify key metrics
    assert_eq!(result.operation_metrics.total_operations, test_metrics.operation_metrics.total_operations);
    assert_eq!(result.resource_metrics.memory_usage, test_metrics.resource_metrics.memory_usage);
    assert_eq!(result.uptime_secs, test_metrics.uptime_secs);
}

// Skipping this test for now as it requires implementing the ComponentType/Lower/Lift traits
/*
#[tokio::test]
async fn test_call_function() {
    let (handle, mut rx) = setup_test_handle().await;
    
    // Test params and result
    let params = TestParams {
        value: "test input".to_string(),
    };
    
    let result = TestResult {
        result: "test output".to_string(),
    };
    
    // Serialize for comparison
    let params_bytes = serde_json::to_vec(&params).unwrap();
    let result_bytes = serde_json::to_vec(&result).unwrap();
    
    // Spawn a task to handle the function call
    let result_clone = result_bytes.clone();
    tokio::spawn(async move {
        if let Some(ActorOperation::CallFunction { name, params: recv_params, response_tx }) = rx.recv().await {
            assert_eq!(name, "test_function");
            assert_eq!(recv_params, params_bytes);
            let _ = response_tx.send(Ok(result_clone));
        }
    });
    
    // Call the function and verify the result
    let call_result: TestResult = handle.call_function("test_function".to_string(), params).await.unwrap();
    assert_eq!(call_result.result, "test output");
}
*/

#[tokio::test]
async fn test_timeout() {
    let (handle, _rx) = setup_test_handle().await;
    
    // We deliberately don't respond to the operation here
    // to test timeout behavior
    let result = handle.get_state().await;
    assert!(matches!(result, Err(ActorError::OperationTimeout(_))));
}

#[tokio::test]
async fn test_shutdown() {
    let (handle, mut rx) = setup_test_handle().await;
    
    tokio::spawn(async move {
        if let Some(ActorOperation::Shutdown { response_tx }) = rx.recv().await {
            let _ = response_tx.send(Ok(()));
        }
    });
    
    let result = handle.shutdown().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_get_chain() {
    let (handle, mut rx) = setup_test_handle().await;
    
    // Create test chain events
    let mut events = vec![];
    for i in 0..3 {
        events.push(ChainEvent {
            hash: vec![i as u8, (i+1) as u8],
            parent_hash: if i == 0 { None } else { Some(vec![(i-1) as u8, i as u8]) },
            event_type: format!("event-{}", i),
            data: vec![10, 20, 30],
            timestamp: 1000 + i as u64,
            description: Some(format!("Event {}", i)),
        });
    }
    
    let events_clone = events.clone();
    tokio::spawn(async move {
        if let Some(ActorOperation::GetChain { response_tx }) = rx.recv().await {
            let _ = response_tx.send(Ok(events_clone));
        }
    });
    
    // Call get_chain and verify
    let result = handle.get_chain().await.unwrap();
    assert_eq!(result.len(), 3);
    assert_eq!(result[0].event_type, "event-0");
    assert_eq!(result[1].event_type, "event-1");
    assert_eq!(result[2].event_type, "event-2");
}
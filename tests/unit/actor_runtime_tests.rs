use std::time::Duration;

use theater::actor_executor::ActorOperation;
use theater::actor_runtime::StartActorResult;
use theater::actor_store::ActorStore;
use theater::config::{HandlerConfig, InterfacesConfig, ManifestConfig, MessageServerConfig};
use theater::id::TheaterId;
use theater::messages::{ActorMessage, ActorSend, TheaterCommand};
use theater::metrics::{ActorMetrics, OperationMetrics, ResourceMetrics};
use theater::shutdown::ShutdownController;
use tokio::sync::mpsc;

/// Helper to create a test manifest
fn create_test_manifest() -> ManifestConfig {
    ManifestConfig {
        name: "test-actor".to_string(),
        component_path: "test_component.wasm".to_string(),
        init_state: None,
        interface: InterfacesConfig::default(),
        handlers: vec![HandlerConfig::MessageServer(MessageServerConfig {})],
        logging: Default::default(),
        event_server: None,
    }
}

/// Helper to create test metrics
fn create_test_metrics() -> ActorMetrics {
    ActorMetrics {
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
        last_update: Some(std::time::SystemTime::now()),
        uptime_secs: 60,
        start_time: std::time::SystemTime::now(),
    }
}

/// A basic test setup for ActorRuntime
/// Note: This test is marked as ignored because it requires more complex mocking of the
/// ActorComponent creation process which we'll implement in the future
#[tokio::test]
#[ignore]
async fn test_actor_runtime_basic() {
    // Create basic test components
    let actor_id = TheaterId::generate();
    let _config = create_test_manifest();

    let (theater_tx, mut theater_rx) = mpsc::channel(10);
    let (actor_tx, _actor_rx) = mpsc::channel::<ActorMessage>(10);
    let (op_tx, _op_rx) = mpsc::channel::<ActorOperation>(10);
    let (shutdown_controller, _shutdown_receiver) = ShutdownController::new();
    let (_result_tx, _result_rx) = mpsc::channel::<StartActorResult>(1);

    // Set up a monitor for TheaterCommands
    tokio::spawn(async move {
        while let Some(cmd) = theater_rx.recv().await {
            match cmd {
                TheaterCommand::NewEvent { actor_id, event } => {
                    println!("New event from actor {}: {:?}", actor_id, event);
                }
                _ => println!("Other theater command: {:?}", cmd),
            }
        }
    });

    // TODO: Implement actual test with ActorRuntime
    // This would require more complex mocking of the component creation process

    // For now, just verify the basics
    let actor_handle = theater::actor_handle::ActorHandle::new(op_tx.clone());
    let _actor_store = ActorStore::new(actor_id.clone(), theater_tx.clone(), actor_handle.clone());

    // Send a test message
    let test_message = ActorMessage::Send(ActorSend {
        data: b"test message".to_vec(),
    });

    actor_tx.send(test_message).await.unwrap();

    // Signal shutdown
    shutdown_controller.signal_shutdown();

    // Wait a bit for shutdown to propagate
    tokio::time::sleep(Duration::from_millis(100)).await;
}

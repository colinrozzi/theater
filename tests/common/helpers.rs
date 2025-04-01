use anyhow::Result;
use chrono::Utc;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use theater::actor_executor::ActorOperation;
use theater::actor_handle::ActorHandle;
use theater::actor_runtime::{ActorRuntime, StartActorResult};
use theater::actor_store::ActorStore;
use theater::chain::{ChainEvent, StateChain};
use theater::config::{HandlerConfig, ManifestConfig, MessageServerConfig};
use theater::events::{ChainEventData, EventData};
use theater::id::TheaterId;
use theater::messages::{ActorMessage, TheaterCommand};
use theater::shutdown::{ShutdownController, ShutdownReceiver};
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;

/// Create a test event data object for testing
pub fn create_test_event_data(event_type: &str, data: &[u8]) -> ChainEventData {
    ChainEventData {
        event_type: event_type.to_string(),
        data: EventData::Raw(data.to_vec()),
        timestamp: Utc::now().timestamp_millis() as u64,
        description: Some(format!("Test event: {}", event_type)),
    }
}

/// Create a basic test actor manifest
pub fn create_test_manifest(name: &str) -> ManifestConfig {
    let mut config = ManifestConfig {
        name: name.to_string(),
        component_path: format!("{}.wasm", name),
        interface: Default::default(),
        handlers: Vec::new(),
        init_state: None,
        environment: HashMap::new(),
    };
    
    // Add a message server handler
    config.handlers.push(HandlerConfig::MessageServer(MessageServerConfig {
        port: None, // Use ephemeral port
    }));
    
    config
}

/// Create a test state chain
pub async fn create_test_chain(actor_id: TheaterId, num_events: usize) -> StateChain {
    let (tx, _) = mpsc::channel(10);
    let mut chain = StateChain::new(actor_id, tx);
    
    // Add events to the chain
    for i in 0..num_events {
        let data = format!("event data {}", i);
        let event_data = create_test_event_data(&format!("event-{}", i), data.as_bytes());
        chain.add_typed_event(event_data).unwrap();
    }
    
    chain
}

/// Setup for an actor test
pub async fn setup_actor_test() -> (
    TheaterId,
    mpsc::Sender<TheaterCommand>,
    mpsc::Sender<ActorMessage>,
    mpsc::Receiver<ActorMessage>,
    mpsc::Sender<ActorOperation>,
    mpsc::Receiver<ActorOperation>,
    ShutdownController,
    ShutdownReceiver
) {
    let actor_id = TheaterId::generate();
    
    let (theater_tx, _) = mpsc::channel(10);
    let (actor_tx, actor_rx) = mpsc::channel(10);
    let (op_tx, op_rx) = mpsc::channel(10);
    let (shutdown_controller, shutdown_receiver) = ShutdownController::new();
    
    (
        actor_id,
        theater_tx,
        actor_tx,
        actor_rx,
        op_tx,
        op_rx,
        shutdown_controller,
        shutdown_receiver
    )
}
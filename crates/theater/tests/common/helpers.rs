use chrono::Utc;
use theater::ActorOperation;
use theater::chain::StateChain;
use theater::{HandlerConfig, ManifestConfig, MessageServerConfig};
use theater::events::message::MessageEventData;
use theater::events::{ChainEventData, EventData};
use theater::id::TheaterId;
use theater::messages::{ActorMessage, TheaterCommand};
use theater::{ShutdownController, ShutdownReceiver};
use tokio::sync::mpsc;

/// Create a test event data object for testing
pub fn create_test_event_data(event_type: &str, data: &[u8]) -> ChainEventData {
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

/// Create a basic test actor manifest
pub fn create_test_manifest(name: &str) -> ManifestConfig {
    let mut config = ManifestConfig {
        name: name.to_string(),
        version: "1.0.0".to_string(),
        component: format!("{}.wasm", name),
        description: None,
        long_description: None,
        save_chain: None,
        permission_policy: Default::default(),
        init_state: None,
        handlers: Vec::new(),
    };

    // Add a message server handler
    config
        .handlers
        .push(HandlerConfig::MessageServer { config: MessageServerConfig {} });

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
    ShutdownReceiver,
) {
    let actor_id = TheaterId::generate();

    let (theater_tx, _) = mpsc::channel(10);
    let (actor_tx, actor_rx) = mpsc::channel(10);
    let (op_tx, op_rx) = mpsc::channel(10);
    let mut shutdown_controller = ShutdownController::new();
    let shutdown_receiver = shutdown_controller.subscribe();

    (
        actor_id,
        theater_tx,
        actor_tx,
        actor_rx,
        op_tx,
        op_rx,
        shutdown_controller,
        shutdown_receiver,
    )
}

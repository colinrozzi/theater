use packr::{link, Layout, LinkBinary, DEFAULT_ALLOCATOR_WASM};
use theater::chain::StateChain;
use theater::events::wasm::WasmEventData;
use theater::events::{ChainEventData, ChainEventPayload};
use theater::id::TheaterId;
use theater::messages::{ActorMessage, TheaterCommand};
use theater::ActorOperation;
use theater::{HandlerConfig, ManifestConfig, MessageServerConfig};
use theater::{ShutdownController, ShutdownReceiver};
use tokio::sync::mpsc;

/// Link a cargo-built actor member + the packr **bundled** allocator
/// (`DEFAULT_ALLOCATOR_WASM`) into a self-contained composite loadable by the
/// 0.10.x self-contained loader.
///
/// Test fixtures are single-package: no `[[link]]` edges, so the composite's
/// residual imports are exactly the actor's host interfaces (`theater:simple/*`);
/// `pack:alloc` + the memory are internalized. The member must be built with the
/// fixed-base recipe (see any `test-actors/*/.cargo/config.toml`).
///
/// Requires `wasm-merge` (binaryen) on PATH — `packr::link` shells out to it.
pub fn link_self_contained(member: Vec<u8>) -> Vec<u8> {
    link(
        vec![
            LinkBinary {
                alias: "alloc".into(),
                wasm: DEFAULT_ALLOCATOR_WASM.to_vec(),
                allocator: true,
            },
            LinkBinary {
                alias: "actor".into(),
                wasm: member,
                allocator: false,
            },
        ],
        &[],
        Layout::default(),
    )
    .expect("link actor member + bundled allocator into a self-contained composite")
}

/// Create a test event data object for testing
pub fn create_test_event_data(event_type: &str, _data: &[u8]) -> ChainEventData {
    ChainEventData {
        event_type: event_type.to_string(),
        data: ChainEventPayload::Wasm(WasmEventData::WasmCall {
            function_name: "test-function".to_string(),
            params: theater::Value::Tuple(vec![]),
        }),
    }
}

/// Create a basic test actor manifest
pub fn create_test_manifest(name: &str) -> ManifestConfig {
    let mut config = ManifestConfig {
        name: name.to_string(),
        version: "1.0.0".to_string(),
        package: format!("{}.wasm", name),
        description: None,
        long_description: None,
        initial_state: None,
        static_package: false,
        permission_policy: Default::default(),
        handlers: Vec::new(),
    };

    // Add a message server handler
    config.handlers.push(HandlerConfig::MessageServer {
        config: MessageServerConfig {},
    });

    config
}

/// Create a test state chain
pub async fn create_test_chain(actor_id: TheaterId, num_events: usize) -> StateChain {
    let mut chain = StateChain::new(actor_id);

    // Add events to the chain
    for i in 0..num_events {
        let data = format!("event data {}", i);
        let event_data = create_test_event_data(&format!("event-{}", i), data.as_bytes());
        chain.add_typed_event(event_data).await.unwrap();
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

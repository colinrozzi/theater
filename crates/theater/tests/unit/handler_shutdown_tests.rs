//! Handler Shutdown Tests
//!
//! These tests verify that handlers properly respond to shutdown signals and
//! clean up their resources. Each handler should:
//! 1. Complete its setup() future when shutdown is signaled
//! 2. Clean up any background tasks
//! 3. Release any held resources

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};

use theater::actor::handle::ActorHandle;
use theater::actor::types::{ActorControl, ActorInfo, ActorOperation};
use theater::config::actor_manifest::{RuntimeHostConfig, SupervisorHostConfig};
use theater::handler::{Handler, SharedActorInstance};
use theater::messages::TheaterCommand;
use theater::shutdown::{ShutdownController, ShutdownType};

/// Timeout for handler shutdown tests
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

/// Create a mock ActorHandle for testing
fn create_mock_actor_handle() -> (
    ActorHandle,
    mpsc::Receiver<ActorOperation>,
    mpsc::Receiver<ActorInfo>,
    mpsc::Receiver<ActorControl>,
) {
    let (operation_tx, operation_rx) = mpsc::channel(10);
    let (info_tx, info_rx) = mpsc::channel(10);
    let (control_tx, control_rx) = mpsc::channel(10);

    let handle = ActorHandle::new(operation_tx, info_tx, control_tx);
    (handle, operation_rx, info_rx, control_rx)
}

/// Create a mock SharedActorInstance
fn create_mock_actor_instance() -> SharedActorInstance {
    Arc::new(RwLock::new(None))
}

/// Create a mock event broadcast channel
fn create_mock_event_rx() -> tokio::sync::broadcast::Receiver<theater::chain::ChainEvent> {
    let (tx, rx) = tokio::sync::broadcast::channel(10);
    drop(tx); // We don't need the sender for these tests
    rx
}

/// Test helper that verifies a handler's setup() completes on shutdown
async fn verify_handler_shutdown<H: Handler>(
    mut handler: H,
    handler_name: &str,
) -> anyhow::Result<()> {
    let (actor_handle, _op_rx, _info_rx, _ctrl_rx) = create_mock_actor_handle();
    let actor_instance = create_mock_actor_instance();
    let event_rx = create_mock_event_rx();

    let mut shutdown_controller = ShutdownController::new();
    let shutdown_receiver = shutdown_controller.subscribe();

    // Start the handler's setup future
    let setup_future = handler.setup(actor_handle, actor_instance, shutdown_receiver, event_rx);

    // Spawn setup in a task
    let setup_handle = tokio::spawn(setup_future);

    // Give the setup a moment to start
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Signal shutdown
    let shutdown_future = shutdown_controller.signal_shutdown(ShutdownType::Graceful);

    // Wait for both shutdown signaling and setup to complete
    let result = tokio::time::timeout(SHUTDOWN_TIMEOUT, async {
        shutdown_future.await;
        setup_handle.await
    })
    .await;

    match result {
        Ok(Ok(Ok(()))) => {
            println!("{} handler shutdown: OK", handler_name);
            Ok(())
        }
        Ok(Ok(Err(e))) => {
            anyhow::bail!("{} handler setup returned error: {:?}", handler_name, e)
        }
        Ok(Err(e)) => {
            anyhow::bail!("{} handler setup task panicked: {:?}", handler_name, e)
        }
        Err(_) => {
            anyhow::bail!(
                "{} handler did not shutdown within {:?}",
                handler_name,
                SHUTDOWN_TIMEOUT
            )
        }
    }
}

// ============================================================================
// Runtime Handler Tests
// ============================================================================

#[tokio::test]
async fn test_runtime_handler_shutdown() {
    use theater_handler_runtime::RuntimeHandler;

    let (theater_tx, _): (mpsc::Sender<TheaterCommand>, _) = mpsc::channel(10);
    let config = RuntimeHostConfig {};
    let handler = RuntimeHandler::new(config, theater_tx, None);

    verify_handler_shutdown(handler, "Runtime")
        .await
        .expect("Runtime handler should shutdown cleanly");
}

// ============================================================================
// Store Handler Tests
// ============================================================================

#[tokio::test]
async fn test_store_handler_shutdown() {
    use theater::config::actor_manifest::StoreHandlerConfig;
    use theater_handler_store::StoreHandler;

    let config = StoreHandlerConfig::default();
    let handler = StoreHandler::new(config, None);

    verify_handler_shutdown(handler, "Store")
        .await
        .expect("Store handler should shutdown cleanly");
}

// ============================================================================
// RPC Handler Tests
// ============================================================================

#[tokio::test]
async fn test_rpc_handler_shutdown() {
    use theater_handler_rpc::RpcHandler;

    let (theater_tx, _): (mpsc::Sender<TheaterCommand>, _) = mpsc::channel(10);
    let handler = RpcHandler::new(theater_tx);

    verify_handler_shutdown(handler, "RPC")
        .await
        .expect("RPC handler should shutdown cleanly");
}

// ============================================================================
// Timer Handler Tests
// ============================================================================

#[tokio::test]
async fn test_timer_handler_shutdown() {
    use theater::config::actor_manifest::TimerHandlerConfig;
    use theater_handler_timer::TimerHandler;

    let config = TimerHandlerConfig::default();
    let handler = TimerHandler::new(config);

    verify_handler_shutdown(handler, "Timer")
        .await
        .expect("Timer handler should shutdown cleanly");
}

// ============================================================================
// TCP Handler Tests
// ============================================================================

#[tokio::test]
async fn test_tcp_handler_shutdown() {
    use theater::config::actor_manifest::TcpHandlerConfig;
    use theater_handler_tcp::TcpHandler;

    let config = TcpHandlerConfig::default();
    let handler = TcpHandler::new(config);

    verify_handler_shutdown(handler, "TCP")
        .await
        .expect("TCP handler should shutdown cleanly");
}

// ============================================================================
// Loop Handler Tests
// ============================================================================

#[tokio::test]
async fn test_loop_handler_shutdown_without_start() {
    use theater_handler_loop::LoopHandler;

    let handler = LoopHandler::new();

    // This tests the case where start-loop() is never called
    // The handler should still shutdown cleanly
    verify_handler_shutdown(handler, "Loop (without start)")
        .await
        .expect("Loop handler should shutdown cleanly even without start-loop()");
}

// ============================================================================
// Terminal Handler Tests
// ============================================================================

#[tokio::test]
async fn test_terminal_handler_shutdown_without_enable_input() {
    use theater::config::actor_manifest::TerminalHandlerConfig;
    use theater_handler_terminal::TerminalHandler;

    let config = TerminalHandlerConfig::default();
    let handler = TerminalHandler::new(config);

    // This tests the case where enable-input() is never called
    // The handler should still shutdown cleanly
    verify_handler_shutdown(handler, "Terminal (without enable-input)")
        .await
        .expect("Terminal handler should shutdown cleanly even without enable-input()");
}

// ============================================================================
// Message Server Handler Tests
// ============================================================================

#[tokio::test]
async fn test_message_server_handler_shutdown_without_register() {
    use theater_handler_message_server::{MessageRouter, MessageServerHandler};

    let router = MessageRouter::new();
    let handler = MessageServerHandler::new(None, router);

    // This tests the case where register() is never called
    // Before the fix, this would hang forever
    verify_handler_shutdown(handler, "MessageServer (without register)")
        .await
        .expect("MessageServer handler should shutdown cleanly even without register()");
}

// ============================================================================
// Supervisor Handler Tests
// ============================================================================

#[tokio::test]
async fn test_supervisor_handler_shutdown() {
    use theater_handler_supervisor::SupervisorHandler;

    let config = SupervisorHostConfig {};
    let handler = SupervisorHandler::new(config, None);

    verify_handler_shutdown(handler, "Supervisor")
        .await
        .expect("Supervisor handler should shutdown cleanly");
}

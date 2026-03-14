//! # Terminal Handler
//!
//! Provides terminal I/O capabilities to WebAssembly actors in the Theater system.
//! Enables building interactive CLI applications, REPLs, and TUI apps.

use std::future::Future;
use std::io::{self, Write};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::io::{AsyncReadExt, BufReader};
use tokio::sync::{Mutex, Notify};
use tracing::{debug, error, info, warn};

use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::config::actor_manifest::{HandlerConfig, TerminalHandlerConfig};
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::shutdown::ShutdownReceiver;


use theater::pack_bridge::{
    parse_pact, AsyncCtx, HostLinkerBuilder, InterfaceImpl, LinkerError, TypeHash, Value,
    ValueType,
};

// ============================================================================
// Interface Declarations
// ============================================================================

/// Embedded terminal.pact file content
const TERMINAL_PACT: &str = include_str!("../../../pact/terminal.pact");

/// Declare the theater:simple/terminal interface from the pact file.
fn terminal_interface() -> InterfaceImpl {
    let pact = parse_pact(TERMINAL_PACT).expect("embedded terminal.pact should be valid");
    InterfaceImpl::from_pact(&pact)
}

// ============================================================================
// Terminal State
// ============================================================================

/// Shared terminal state
#[derive(Clone)]
struct TerminalState {
    /// Whether raw mode is enabled
    raw_mode: Arc<AtomicBool>,
    /// Original termios settings (for restoring on exit)
    #[cfg(unix)]
    original_termios: Arc<Mutex<Option<libc::termios>>>,
}

impl TerminalState {
    fn new() -> Self {
        Self {
            raw_mode: Arc::new(AtomicBool::new(false)),
            #[cfg(unix)]
            original_termios: Arc::new(Mutex::new(None)),
        }
    }

    #[cfg(unix)]
    async fn set_raw_mode(&self, enabled: bool) -> Result<(), String> {
        use std::os::unix::io::AsRawFd;

        let stdin_fd = io::stdin().as_raw_fd();

        if enabled {
            // Save original settings
            let mut termios: libc::termios = unsafe { std::mem::zeroed() };
            if unsafe { libc::tcgetattr(stdin_fd, &mut termios) } != 0 {
                return Err("Failed to get terminal attributes".to_string());
            }

            {
                let mut original = self.original_termios.lock().await;
                if original.is_none() {
                    *original = Some(termios);
                }
            }

            // Enable raw mode
            let mut raw = termios;
            unsafe {
                libc::cfmakeraw(&mut raw);
            }
            if unsafe { libc::tcsetattr(stdin_fd, libc::TCSANOW, &raw) } != 0 {
                return Err("Failed to set raw mode".to_string());
            }

            self.raw_mode.store(true, Ordering::SeqCst);
            debug!("Raw mode enabled");
        } else {
            // Restore original settings
            let original = self.original_termios.lock().await;
            if let Some(ref termios) = *original {
                if unsafe { libc::tcsetattr(stdin_fd, libc::TCSANOW, termios) } != 0 {
                    return Err("Failed to restore terminal attributes".to_string());
                }
            }
            self.raw_mode.store(false, Ordering::SeqCst);
            debug!("Raw mode disabled");
        }

        Ok(())
    }

    #[cfg(not(unix))]
    async fn set_raw_mode(&self, enabled: bool) -> Result<(), String> {
        // On non-Unix platforms, just track the state
        self.raw_mode.store(enabled, Ordering::SeqCst);
        Ok(())
    }

    fn get_size() -> Result<(u16, u16), String> {
        #[cfg(unix)]
        {
            let mut size: libc::winsize = unsafe { std::mem::zeroed() };
            let stdout_fd = libc::STDOUT_FILENO;

            if unsafe { libc::ioctl(stdout_fd, libc::TIOCGWINSZ, &mut size) } != 0 {
                return Err("Failed to get terminal size".to_string());
            }

            Ok((size.ws_col, size.ws_row))
        }

        #[cfg(not(unix))]
        {
            // Default fallback
            Ok((80, 24))
        }
    }

    #[cfg(unix)]
    async fn restore_terminal(&self) {
        use std::os::unix::io::AsRawFd;

        let stdin_fd = io::stdin().as_raw_fd();
        let original = self.original_termios.lock().await;
        if let Some(ref termios) = *original {
            unsafe {
                libc::tcsetattr(stdin_fd, libc::TCSANOW, termios);
            }
        }
    }

    #[cfg(not(unix))]
    async fn restore_terminal(&self) {
        // Nothing to restore on non-Unix
    }
}

// ============================================================================
// Handler Implementation
// ============================================================================

/// Handler for providing terminal I/O to WebAssembly actors.
#[derive(Clone)]
pub struct TerminalHandler {
    config: TerminalHandlerConfig,
    state: Option<TerminalState>,
    actor_handle: Arc<std::sync::Mutex<Option<ActorHandle>>>,
    /// Shutdown receiver for background input loop (used by enable-input())
    shutdown_receiver: Arc<std::sync::Mutex<Option<ShutdownReceiver>>>,
    /// Shutdown receiver for setup() itself - ensures setup() can exit on shutdown
    setup_shutdown_receiver: Arc<std::sync::Mutex<Option<ShutdownReceiver>>>,
    /// Notifies setup() when enable-input() takes the shutdown receiver
    input_enabled_notify: Arc<Notify>,
}

impl TerminalHandler {
    pub fn new(config: TerminalHandlerConfig) -> Self {
        Self {
            config,
            state: None,
            actor_handle: Arc::new(std::sync::Mutex::new(None)),
            shutdown_receiver: Arc::new(std::sync::Mutex::new(None)),
            setup_shutdown_receiver: Arc::new(std::sync::Mutex::new(None)),
            input_enabled_notify: Arc::new(Notify::new()),
        }
    }
}

impl Handler for TerminalHandler {
    fn create_instance(&self, config: Option<&HandlerConfig>) -> Box<dyn Handler> {
        let terminal_config = match config {
            Some(HandlerConfig::Terminal { config }) => config.clone(),
            _ => self.config.clone(),
        };
        Box::new(TerminalHandler::new(terminal_config))
    }

    fn name(&self) -> &str {
        "terminal"
    }

    fn imports(&self) -> Option<Vec<String>> {
        Some(
            self.interfaces()
                .iter()
                .map(|i| i.name().to_string())
                .collect(),
        )
    }

    fn exports(&self) -> Option<Vec<String>> {
        Some(vec!["theater:simple/terminal".to_string()])
    }

    fn interface_hashes(&self) -> Vec<(String, TypeHash)> {
        self.interfaces()
            .iter()
            .map(|i| (i.name().to_string(), i.hash()))
            .collect()
    }

    fn interfaces(&self) -> Vec<InterfaceImpl> {
        vec![terminal_interface()]
    }

    fn setup(
        &mut self,
        actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance,
        shutdown_receiver: ShutdownReceiver,
        _event_rx: tokio::sync::broadcast::Receiver<theater::chain::ChainEvent>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        info!("Terminal handler setup (passive mode)");

        // Store the actor_handle for use by enable-input()
        {
            let mut handle_guard = self.actor_handle.lock().unwrap();
            *handle_guard = Some(actor_handle);
        }

        // Try to get setup_shutdown_receiver from setup_host_functions_composite
        let setup_receiver = {
            let mut guard = self.setup_shutdown_receiver.lock().unwrap();
            guard.take()
        };

        let input_enabled_notify = self.input_enabled_notify.clone();

        // Use either the setup receiver (from setup_host_functions_composite) or
        // the passed shutdown_receiver directly for setup
        let mut receiver_for_setup = match setup_receiver {
            Some(r) => {
                // We have a dedicated setup receiver, so store the passed one for enable-input
                let mut shutdown_guard = self.shutdown_receiver.lock().unwrap();
                if shutdown_guard.is_none() {
                    *shutdown_guard = Some(shutdown_receiver);
                }
                r
            }
            None => {
                // No setup receiver available, use the passed one directly
                shutdown_receiver
            }
        };

        // Wait for either:
        // 1. enable-input() is called (notified via input_enabled_notify)
        // 2. Shutdown signal received (if enable-input() was never called)
        Box::pin(async move {
            tokio::select! {
                _ = input_enabled_notify.notified() => {
                    info!("Terminal handler: input enabled, setup complete");
                }
                _ = &mut receiver_for_setup.receiver => {
                    info!("Terminal handler: shutdown before enable-input, exiting setup");
                }
            }
            Ok(())
        })
    }

    fn setup_host_functions_composite(
        &mut self,
        builder: &mut HostLinkerBuilder<'_, ActorStore>,
        ctx: &mut HandlerContext,
    ) -> Result<(), LinkerError> {
        info!("Setting up terminal host functions");

        if ctx.is_satisfied("theater:simple/terminal") {
            info!("theater:simple/terminal already satisfied, skipping");
            return Ok(());
        }

        // Subscribe to two shutdown receivers:
        // 1. One for enable-input() to use in the input reading task
        // 2. One for setup() to use so it can exit if enable-input() is never called
        if let Some(shutdown_receiver) = ctx.subscribe_shutdown() {
            let mut guard = self.shutdown_receiver.lock().unwrap();
            *guard = Some(shutdown_receiver);
        }
        if let Some(setup_shutdown_receiver) = ctx.subscribe_shutdown() {
            let mut guard = self.setup_shutdown_receiver.lock().unwrap();
            *guard = Some(setup_shutdown_receiver);
        }

        let state = TerminalState::new();
        self.state = Some(state.clone());

        let st_write_stdout = state.clone();
        let st_write_stderr = state.clone();
        let st_set_raw = state.clone();

        builder
            .interface("theater:simple/terminal")?
            // write-stdout(data: list<u8>) -> result<u64, string>
            .func_async_result(
                "write-stdout",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let _st = st_write_stdout.clone();
                    async move {
                        let data = parse_bytes(&input)?;
                        let mut stdout = io::stdout().lock();
                        stdout
                            .write_all(&data)
                            .map_err(|e| Value::String(e.to_string()))?;
                        stdout.flush().map_err(|e| Value::String(e.to_string()))?;
                        Ok::<Value, Value>(Value::U64(data.len() as u64))
                    }
                },
            )?
            // write-stderr(data: list<u8>) -> result<u64, string>
            .func_async_result(
                "write-stderr",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let _st = st_write_stderr.clone();
                    async move {
                        let data = parse_bytes(&input)?;
                        let mut stderr = io::stderr().lock();
                        stderr
                            .write_all(&data)
                            .map_err(|e| Value::String(e.to_string()))?;
                        stderr.flush().map_err(|e| Value::String(e.to_string()))?;
                        Ok::<Value, Value>(Value::U64(data.len() as u64))
                    }
                },
            )?
            // set-raw-mode(enabled: bool) -> result<_, string>
            .func_async_result(
                "set-raw-mode",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let st = st_set_raw.clone();
                    async move {
                        let enabled = parse_bool(&input)?;
                        st.set_raw_mode(enabled).await.map_err(Value::String)?;
                        Ok::<Value, Value>(Value::Tuple(vec![]))
                    }
                },
            )?
            // get-size() -> result<tuple<u16, u16>, string>
            .func_async_result(
                "get-size",
                move |_ctx: AsyncCtx<ActorStore>, _input: Value| async move {
                    let (cols, rows) = TerminalState::get_size().map_err(Value::String)?;
                    Ok::<Value, Value>(Value::Tuple(vec![Value::U16(cols), Value::U16(rows)]))
                },
            )?
            // enable-input() -> result<_, string>
            // Starts the background input loop that reads from stdin and calls handle-input
            .func_async_result(
                "enable-input",
                {
                    let actor_handle = self.actor_handle.clone();
                    let shutdown_receiver = self.shutdown_receiver.clone();
                    let input_enabled_notify = self.input_enabled_notify.clone();
                    let state = state.clone();
                    move |_ctx: AsyncCtx<ActorStore>, _input: Value| {
                        let actor_handle = actor_handle.clone();
                        let shutdown_receiver = shutdown_receiver.clone();
                        let input_enabled_notify = input_enabled_notify.clone();
                        let state = state.clone();
                        async move {
                            // Get the actor handle
                            let handle = {
                                let guard = actor_handle.lock().unwrap();
                                guard.clone().ok_or_else(|| {
                                    Value::String("Actor handle not available".to_string())
                                })?
                            };

                            // Get the shutdown receiver
                            let shutdown_rx = {
                                let mut guard = shutdown_receiver.lock().unwrap();
                                guard.take().ok_or_else(|| {
                                    Value::String("Input already enabled".to_string())
                                })?
                            };

                            // Notify setup() that we've taken the receiver
                            input_enabled_notify.notify_one();

                            // Spawn the input loop as a background task
                            tokio::spawn(run_input_loop(handle, shutdown_rx, state));

                            info!("Terminal input enabled");
                            Ok::<Value, Value>(Value::Tuple(vec![]))
                        }
                    }
                },
            )?;

        ctx.mark_satisfied("theater:simple/terminal");
        info!("Terminal host functions registered");

        Ok(())
    }
}

// ============================================================================
// Input Loop
// ============================================================================

/// Background task that reads stdin and signals, calling actor export functions
async fn run_input_loop(
    actor_handle: ActorHandle,
    shutdown_receiver: ShutdownReceiver,
    state: TerminalState,
) {
    // Set up signal handling
    #[cfg(unix)]
    let mut signals = {
        use signal_hook::consts::signal::{SIGINT, SIGTERM, SIGWINCH};
        use signal_hook_tokio::Signals;

        Signals::new([SIGINT, SIGTERM, SIGWINCH]).expect("Failed to register signal handlers")
    };

    // Set up stdin reader
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut buffer = vec![0u8; 1024];

    // Create fused shutdown future
    let mut shutdown_fut = std::pin::pin!(shutdown_receiver.wait_for_shutdown());
    let mut shutdown_complete = false;

    // Main event loop
    loop {
        if shutdown_complete {
            break;
        }

        #[cfg(unix)]
        {
            use futures_util::StreamExt;

            tokio::select! {
                biased;

                // Shutdown signal (only poll if not already triggered)
                _ = &mut shutdown_fut, if !shutdown_complete => {
                    info!("Terminal input loop received shutdown");
                    shutdown_complete = true;
                }

                // Stdin input
                result = reader.read(&mut buffer) => {
                    match result {
                        Ok(0) => {
                            info!("Stdin closed (EOF)");
                            break;
                        }
                        Ok(n) => {
                            let data = buffer[..n].to_vec();
                            debug!("Read {} bytes from stdin", n);

                            let input_value = Value::List {
                                elem_type: ValueType::U8,
                                items: data.iter().map(|&b| Value::U8(b)).collect(),
                            };

                            if let Err(e) = actor_handle
                                .call_function(
                                    "theater:simple/terminal.handle-input".to_string(),
                                    input_value,
                                )
                                .await
                            {
                                error!("Failed to call handle-input: {:?}", e);
                            }
                        }
                        Err(e) => {
                            error!("Error reading stdin: {}", e);
                            break;
                        }
                    }
                }

                // Signals
                signal = signals.next() => {
                    use signal_hook::consts::signal::{SIGINT, SIGTERM, SIGWINCH};

                    match signal {
                        Some(SIGINT) => {
                            debug!("Received SIGINT");
                            let input = Value::String("interrupt".to_string());
                            if let Err(e) = actor_handle
                                .call_function(
                                    "theater:simple/terminal.handle-signal".to_string(),
                                    input,
                                )
                                .await
                            {
                                warn!("Failed to call handle-signal: {:?}", e);
                            }
                        }
                        Some(SIGTERM) => {
                            debug!("Received SIGTERM");
                            let input = Value::String("terminate".to_string());
                            if let Err(e) = actor_handle
                                .call_function(
                                    "theater:simple/terminal.handle-signal".to_string(),
                                    input,
                                )
                                .await
                            {
                                warn!("Failed to call handle-signal: {:?}", e);
                            }
                        }
                        Some(SIGWINCH) => {
                            debug!("Received SIGWINCH");
                            if let Ok((cols, rows)) = TerminalState::get_size() {
                                let input = Value::Tuple(vec![Value::U16(cols), Value::U16(rows)]);
                                if let Err(e) = actor_handle
                                    .call_function(
                                        "theater:simple/terminal.handle-resize".to_string(),
                                        input,
                                    )
                                    .await
                                {
                                    warn!("Failed to call handle-resize: {:?}", e);
                                }
                            }
                        }
                        Some(sig) => {
                            debug!("Received signal {}", sig);
                        }
                        None => {
                            break;
                        }
                    }
                }
            }
        }

        #[cfg(not(unix))]
        {
            tokio::select! {
                biased;

                _ = &mut shutdown_fut, if !shutdown_complete => {
                    info!("Terminal input loop received shutdown");
                    shutdown_complete = true;
                }

                result = reader.read(&mut buffer) => {
                    match result {
                        Ok(0) => {
                            info!("Stdin closed (EOF)");
                            break;
                        }
                        Ok(n) => {
                            let data = buffer[..n].to_vec();
                            debug!("Read {} bytes from stdin", n);

                            let input_value = Value::List {
                                elem_type: ValueType::U8,
                                items: data.iter().map(|&b| Value::U8(b)).collect(),
                            };

                            if let Err(e) = actor_handle
                                .call_function(
                                    "theater:simple/terminal.handle-input".to_string(),
                                    input_value,
                                )
                                .await
                            {
                                error!("Failed to call handle-input: {:?}", e);
                            }
                        }
                        Err(e) => {
                            error!("Error reading stdin: {}", e);
                            break;
                        }
                    }
                }
            }
        }
    }

    // Restore terminal on exit
    state.restore_terminal().await;
    info!("Terminal input loop exited");
}

// ============================================================================
// Helpers
// ============================================================================

fn parse_bytes(input: &Value) -> Result<Vec<u8>, Value> {
    match input {
        Value::List { items, .. } => {
            let bytes: Result<Vec<u8>, _> = items
                .iter()
                .map(|v| match v {
                    Value::U8(b) => Ok(*b),
                    _ => Err(Value::String("Expected u8 in list".to_string())),
                })
                .collect();
            bytes
        }
        _ => Err(Value::String("Expected list<u8>".to_string())),
    }
}

fn parse_bool(input: &Value) -> Result<bool, Value> {
    match input {
        Value::Bool(b) => Ok(*b),
        _ => Err(Value::String("Expected bool".to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_interface_parses() {
        let iface = terminal_interface();
        assert_eq!(iface.name(), "theater:simple/terminal");
    }

    #[test]
    fn test_handler_name() {
        let handler = TerminalHandler::new(TerminalHandlerConfig::default());
        assert_eq!(handler.name(), "terminal");
    }
}

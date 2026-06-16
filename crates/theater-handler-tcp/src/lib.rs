//! # TCP Handler
//!
//! Provides raw TCP networking capabilities to WebAssembly actors in the Theater system.
//! This handler is deliberately minimal — it moves bytes across the boundary and leaves
//! all protocol complexity (framing, routing, addressing) to actor-space code.
//!
//! ## Connection Handoff
//!
//! Connections can be transferred between actors for the "accept and hand off" pattern:
//!
//! 1. Acceptor calls `accept()` - connection starts in PENDING state
//! 2. Acceptor spawns a worker actor
//! 3. Acceptor calls `transfer(conn_id, worker_id)` - atomically transfers and activates
//! 4. Worker receives `handle-connection` callback and can immediately send/receive
//!
//! This prevents race conditions where data arrives before the handoff completes.
//!
//! ## Data Modes (Erlang-style)
//!
//! Connections support three data modes via `set-active()`:
//!
//! - `"passive"` (default): Data received only via explicit `receive()` calls
//! - `"active"`: Data pushed to actor via `on-data` callback continuously
//! - `"once"`: Single `on-data` callback, then switches back to passive
//!
//! This matches Erlang/OTP's `{active, true/false/once}` socket options.
//!
//! ## TLS Support
//!
//! TLS can be enabled via manifest configuration:
//!
//! ```toml
//! [[handler]]
//! type = "tcp"
//!
//! [handler.client_tls]
//! enabled = true
//! # ca_cert = "/path/to/ca.pem"  # Optional custom CA
//! # skip_verify = false          # For development only
//!
//! [handler.server_tls]
//! enabled = true
//! cert = "/path/to/server.pem"
//! key = "/path/to/server-key.pem"
//! ```
//!
//! When TLS is configured, connections are automatically encrypted. The actor
//! code doesn't need to change - it uses the same `tcp-connect`, `tcp-listen`,
//! `tcp-read`, `tcp-write` interface.

mod stream;
mod tls;

use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use stream::{UnifiedReadHalf, UnifiedStream, UnifiedWriteHalf};
use tls::TlsContext;

use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::config::actor_manifest::{HandlerConfig, TcpHandlerConfig};
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::id::TheaterId;
use theater::shutdown::ShutdownReceiver;

use theater::pack_bridge::{
    parse_pact, AsyncCtx, HostLinkerBuilder, InterfaceImpl, LinkerError, TypeHash, Value, ValueType,
};

// ============================================================================
// Interface Declarations
// ============================================================================

/// Drops at scope exit and emits a `phase=... elapsed_ms=...` debug line.
/// One line per host fn invocation, regardless of which return path was
/// taken (incl. `?` short-circuits).
struct PhaseLog {
    name: &'static str,
    start: Instant,
}

impl PhaseLog {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            start: Instant::now(),
        }
    }
}

impl Drop for PhaseLog {
    fn drop(&mut self) {
        debug!(
            phase = self.name,
            elapsed_ms = self.start.elapsed().as_millis() as u64,
            "tcp phase complete",
        );
    }
}

/// Embedded tcp.pact file content
const TCP_PACT: &str = include_str!("../tcp.pact");

/// Declare the theater:simple/tcp interface from the pact file.
fn tcp_interface() -> InterfaceImpl {
    let pact = parse_pact(TCP_PACT).expect("embedded tcp.pact should be valid");
    InterfaceImpl::from_pact(&pact)
}

// ============================================================================
// Connection State
// ============================================================================

/// State of a connection in its lifecycle
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectionState {
    /// Connection accepted but not yet activated - no data operations allowed
    Pending,
    /// Connection is active - send/receive allowed
    Active,
}

/// Data mode for receiving data (Erlang-style)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DataMode {
    /// Data only received via explicit receive() calls
    Passive,
    /// Data pushed to on-data callback continuously
    Active,
    /// Receive one chunk via on-data, then switch to Passive
    Once,
}

/// Represents the stream state based on data mode
enum StreamState {
    /// Full stream available for passive mode operations
    Full(Box<UnifiedStream>),
    /// Only write half available - read half taken by active mode task
    WriteOnly(UnifiedWriteHalf),
    /// Connection closed or stream taken
    Closed,
}

/// A tracked TCP connection with ownership and state.
///
/// `stream` is wrapped in `Arc<Mutex<...>>` so the outer connections map
/// mutex is only held briefly for lookup/metadata. The actual I/O acquires
/// the per-connection lock — this lets two actors do I/O on different
/// connections in parallel, which is essential for any flow where the
/// runtime hosts both sides of a TCP conversation (e.g. an outbound SMTP
/// client talking to a local SMTP server in the same theater instance).
struct ConnectionEntry {
    stream: Arc<Mutex<StreamState>>,
    peer_addr: SocketAddr,
    owner: TheaterId,
    state: ConnectionState,
    data_mode: DataMode,
}

/// A tracked TCP listener with ownership
struct ListenerEntry {
    listener: TcpListener,
    owner: TheaterId,
}

/// Shared TCP state across all actor instances.
///
/// This state is shared via Arc, so all TcpHandler instances in a Theater
/// runtime see the same connections and listeners. This enables connection
/// transfer between actors.
struct SharedTcpState {
    connections: Mutex<HashMap<u64, ConnectionEntry>>,
    listeners: Mutex<HashMap<u64, ListenerEntry>>,
    next_id: AtomicU64,
    max_connections: Option<u32>,
}

impl SharedTcpState {
    fn new(max_connections: Option<u32>) -> Self {
        Self {
            connections: Mutex::new(HashMap::new()),
            listeners: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            max_connections,
        }
    }

    fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    async fn check_connection_limit(&self) -> Result<(), Value> {
        if let Some(max) = self.max_connections {
            let count = self.connections.lock().await.len();
            if count >= max as usize {
                return Err(Value::String(format!(
                    "Connection limit reached ({}/{})",
                    count, max
                )));
            }
        }
        Ok(())
    }
}

// ============================================================================
// Handler Implementation
// ============================================================================

/// Handler for providing raw TCP networking access to WebAssembly actors.
#[derive(Clone)]
pub struct TcpHandler {
    config: TcpHandlerConfig,
    /// Shared state across all handler instances - enables connection transfer
    shared_state: Arc<SharedTcpState>,
    /// Actor ID for this handler instance - set during setup
    actor_id: Arc<std::sync::Mutex<Option<TheaterId>>>,
    /// Actor handle for calling export functions (set in setup, used by listen)
    actor_handle: Arc<std::sync::Mutex<Option<ActorHandle>>>,
    /// Cancellation token for spawned background tasks
    cancellation_token: CancellationToken,
    /// TLS context for encrypted connections (shared across clones)
    tls_context: Arc<Option<TlsContext>>,
}

impl TcpHandler {
    pub fn new(config: TcpHandlerConfig) -> Self {
        // Build TLS context from config
        let tls_context = match TlsContext::from_config(&config) {
            Ok(ctx) => Arc::new(ctx),
            Err(e) => {
                error!("Failed to build TLS context: {}. TLS will be disabled.", e);
                Arc::new(None)
            }
        };

        Self {
            config,
            shared_state: Arc::new(SharedTcpState::new(None)),
            actor_id: Arc::new(std::sync::Mutex::new(None)),
            actor_handle: Arc::new(std::sync::Mutex::new(None)),
            cancellation_token: CancellationToken::new(),
            tls_context,
        }
    }

    /// Get the interface declarations for this handler.
    pub fn interfaces(&self) -> Vec<InterfaceImpl> {
        vec![tcp_interface()]
    }
}

// ── Value parsing helpers ─────────────────────────────────────────────────

fn parse_string(input: &Value) -> Result<String, Value> {
    match input {
        Value::String(s) => Ok(s.clone()),
        Value::Tuple(fields) if fields.len() == 1 => match &fields[0] {
            Value::String(s) => Ok(s.clone()),
            _ => Err(Value::String("Expected string".to_string())),
        },
        _ => Err(Value::String("Expected string".to_string())),
    }
}

fn parse_two_strings(input: &Value) -> Result<(String, String), Value> {
    match input {
        Value::Tuple(fields) if fields.len() == 2 => {
            let a = match &fields[0] {
                Value::String(s) => s.clone(),
                _ => return Err(Value::String("Expected string for first arg".to_string())),
            };
            let b = match &fields[1] {
                Value::String(s) => s.clone(),
                _ => return Err(Value::String("Expected string for second arg".to_string())),
            };
            Ok((a, b))
        }
        _ => Err(Value::String("Expected tuple (string, string)".to_string())),
    }
}

fn parse_string_and_bytes(input: &Value) -> Result<(String, Vec<u8>), Value> {
    match input {
        Value::Tuple(fields) if fields.len() == 2 => {
            let id = match &fields[0] {
                Value::String(s) => s.clone(),
                _ => return Err(Value::String("Expected string for id".to_string())),
            };
            let data = match &fields[1] {
                Value::List { items, .. } => items
                    .iter()
                    .filter_map(|v| match v {
                        Value::U8(b) => Some(*b),
                        _ => None,
                    })
                    .collect::<Vec<u8>>(),
                _ => return Err(Value::String("Expected list<u8> for data".to_string())),
            };
            Ok((id, data))
        }
        _ => Err(Value::String("Expected tuple (id, data)".to_string())),
    }
}

fn parse_string_and_u32(input: &Value) -> Result<(String, u32), Value> {
    match input {
        Value::Tuple(fields) if fields.len() == 2 => {
            let id = match &fields[0] {
                Value::String(s) => s.clone(),
                _ => return Err(Value::String("Expected string for id".to_string())),
            };
            let n = match &fields[1] {
                Value::U32(n) => *n,
                _ => return Err(Value::String("Expected u32".to_string())),
            };
            Ok((id, n))
        }
        _ => Err(Value::String("Expected tuple (id, u32)".to_string())),
    }
}

fn id_to_string(id: u64) -> String {
    id.to_string()
}

fn string_to_id(s: &str) -> Result<u64, Value> {
    s.parse::<u64>()
        .map_err(|_| Value::String(format!("Invalid id: {}", s)))
}

// ── Handler implementation ────────────────────────────────────────────────

impl Handler for TcpHandler {
    fn create_instance(&self, config: Option<&HandlerConfig>) -> Box<dyn Handler> {
        let tcp_config = match config {
            Some(HandlerConfig::Tcp { config }) => config.clone(),
            _ => self.config.clone(),
        };

        // Build TLS context from config if different from current
        let tls_context = if config.is_some() {
            // New config provided, rebuild TLS context
            match TlsContext::from_config(&tcp_config) {
                Ok(ctx) => Arc::new(ctx),
                Err(e) => {
                    error!("Failed to build TLS context: {}. TLS will be disabled.", e);
                    Arc::new(None)
                }
            }
        } else {
            // Reuse existing TLS context
            self.tls_context.clone()
        };

        // Share the same state across all instances - this is the key for transfer!
        // Each instance gets its own cancellation token (cancelled when that actor shuts down)
        Box::new(TcpHandler {
            config: tcp_config,
            shared_state: self.shared_state.clone(), // Same Arc!
            actor_id: Arc::new(std::sync::Mutex::new(None)),
            actor_handle: Arc::new(std::sync::Mutex::new(None)),
            cancellation_token: CancellationToken::new(),
            tls_context,
        })
    }

    fn name(&self) -> &str {
        "tcp"
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
        Some(vec!["theater:simple/tcp-client".to_string()])
    }

    fn interface_hashes(&self) -> Vec<(String, TypeHash)> {
        self.interfaces()
            .iter()
            .map(|i| (i.name().to_string(), i.hash()))
            .collect()
    }

    fn interfaces(&self) -> Vec<theater::pack_bridge::InterfaceImpl> {
        vec![tcp_interface()]
    }

    fn setup(
        &mut self,
        actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance,
        shutdown_receiver: ShutdownReceiver,
        _event_rx: theater::handler::HandlerEventReceiver,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        info!("TCP handler setup (passive mode)");

        // Store the actor_handle for use by listen()
        {
            let mut handle_guard = self.actor_handle.lock().unwrap();
            *handle_guard = Some(actor_handle);
        }

        // Get cancellation token to cancel on shutdown
        let cancel_token = self.cancellation_token.clone();
        let shared_state = self.shared_state.clone();
        let actor_id_for_cleanup = self.actor_id.clone();

        // Wait for shutdown, then clean up all resources OWNED BY THIS ACTOR.
        // The connections + listeners maps are shared across every TcpHandler
        // instance in the runtime — one map, many actors. So the cleanup must
        // filter by owner; clearing the whole map would wipe in-flight
        // connections owned by other actors. For TLS connections it also
        // drives `AsyncWriteExt::shutdown` so close_notify reaches the wire
        // before TCP FIN (the same shape as the `close` host function).
        Box::pin(async move {
            info!("TCP handler setup waiting for shutdown signal");
            shutdown_receiver.wait_for_shutdown().await;
            info!("TCP handler received shutdown, cleaning up resources");

            // Cancel all spawned background tasks (listeners, active mode readers)
            cancel_token.cancel();
            info!("TCP handler cancellation token cancelled");

            let actor_id_val = *actor_id_for_cleanup.lock().unwrap();

            // Collect connection IDs owned by this actor; release the outer
            // lock before driving per-connection shutdowns (those acquire
            // the inner stream mutex and may await).
            let to_remove: Vec<u64> = {
                let connections = shared_state.connections.lock().await;
                connections
                    .iter()
                    .filter(|(_, entry)| Some(entry.owner) == actor_id_val)
                    .map(|(id, _)| *id)
                    .collect()
            };
            let conn_count = to_remove.len();
            for conn_id in to_remove {
                shutdown_write_half_and_remove(&shared_state, conn_id).await;
            }
            if conn_count > 0 {
                info!(
                    "TCP handler closed {} connections owned by actor {:?}",
                    conn_count, actor_id_val
                );
            }

            // Same owner-filter for listeners.
            {
                let mut listeners = shared_state.listeners.lock().await;
                let before = listeners.len();
                listeners.retain(|_, entry| Some(entry.owner) != actor_id_val);
                let removed = before - listeners.len();
                if removed > 0 {
                    info!(
                        "TCP handler closed {} listeners owned by actor {:?}",
                        removed, actor_id_val
                    );
                }
            }

            info!("TCP handler shutdown complete");
            Ok(())
        })
    }

    fn setup_host_functions_composite(
        &mut self,
        builder: &mut HostLinkerBuilder<'_, ActorStore>,
        ctx: &mut HandlerContext,
    ) -> Result<(), LinkerError> {
        info!("Setting up TCP host functions (Pack)");

        if ctx.is_satisfied("theater:simple/tcp") {
            info!("theater:simple/tcp already satisfied, skipping");
            return Ok(());
        }

        // Get actor ID from context
        let actor_id = ctx
            .actor_id
            .expect("actor_id should be set in HandlerContext");

        // Store actor_id for this instance
        {
            let mut id_guard = self.actor_id.lock().unwrap();
            *id_guard = Some(actor_id);
        }

        // Update max_connections if configured
        // Note: We can't easily update the shared state's max_connections here
        // since it's already created. For now, first handler wins.

        let state = self.shared_state.clone();
        let actor_id_for_closures = actor_id;

        // Clone handler fields for use in listen() callback
        let actor_handle_for_listen = self.actor_handle.clone();
        let cancel_token_for_listen = self.cancellation_token.clone();

        // Clone state and actor_id for each closure
        let st_connect = state.clone();
        let aid_connect = actor_id_for_closures;
        let tls_for_connect = self.tls_context.clone();

        let st_listen = state.clone();
        let aid_listen = actor_id_for_closures;
        let tls_for_listen = self.tls_context.clone();

        let st_accept = state.clone();
        let aid_accept = actor_id_for_closures;
        let tls_for_accept = self.tls_context.clone();

        let st_activate = state.clone();
        let aid_activate = actor_id_for_closures;

        let st_set_active = state.clone();
        let aid_set_active = actor_id_for_closures;
        let actor_handle_for_set_active = self.actor_handle.clone();
        let cancel_token_for_set_active = self.cancellation_token.clone();

        let st_transfer = state.clone();
        let aid_transfer = actor_id_for_closures;

        let st_peer = state.clone();
        let aid_peer = actor_id_for_closures;

        let st_send = state.clone();
        let aid_send = actor_id_for_closures;

        let st_receive = state.clone();
        let aid_receive = actor_id_for_closures;
        let cancel_token_for_receive = self.cancellation_token.clone();

        let st_close = state.clone();
        let aid_close = actor_id_for_closures;

        let st_upgrade_server = state.clone();
        let aid_upgrade_server = actor_id_for_closures;
        let tls_for_upgrade_server = self.tls_context.clone();

        let st_upgrade_client = state.clone();
        let aid_upgrade_client = actor_id_for_closures;
        let tls_for_upgrade_client = self.tls_context.clone();

        let st_close_listener = state.clone();
        let aid_close_listener = actor_id_for_closures;

        builder
            .interface("theater:simple/tcp")?
            // ----------------------------------------------------------------
            // connect(address: string) -> result<string, string>
            // Outbound connections are immediately active
            // ----------------------------------------------------------------
            .func_async_result(
                "connect",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let st = st_connect.clone();
                    let actor_id = aid_connect;
                    let tls_ctx = tls_for_connect.clone();
                    async move {
                        let _ph = PhaseLog::new("tcp.connect");
                        let address = parse_string(&input)?;
                        st.check_connection_limit().await?;
                        debug!("tcp connect to {}", address);

                        let tcp_stream = TcpStream::connect(&address)
                            .await
                            .map_err(|e| Value::String(e.to_string()))?;

                        let peer_addr = tcp_stream
                            .peer_addr()
                            .map_err(|e| Value::String(e.to_string()))?;

                        // Apply TLS if configured AND auto_handshake is enabled.
                        // STARTTLS-style protocols set client_tls.auto_handshake = false:
                        // the connector is built (so upgrade-to-tls-client works) but
                        // connect() returns a plain TCP stream until the actor explicitly
                        // upgrades it after negotiating the STARTTLS handshake.
                        let auto_handshake = tls_ctx
                            .as_ref()
                            .as_ref()
                            .map(|c| c.client_auto_handshake)
                            .unwrap_or(true);
                        let unified_stream = if !auto_handshake {
                            UnifiedStream::Plain(tcp_stream)
                        } else if let Some(ref ctx) = *tls_ctx {
                            if let Some(ref connector) = ctx.client_connector {
                                // Extract hostname from address for SNI
                                let server_name = tls::parse_server_name(
                                    address.split(':').next().unwrap_or(&address),
                                )
                                .map_err(|e| Value::String(e.to_string()))?;

                                debug!("tcp connect: performing TLS handshake with SNI {:?}", server_name);
                                let tls_stream = connector
                                    .connect(server_name, tcp_stream)
                                    .await
                                    .map_err(|e| Value::String(format!("TLS handshake failed: {}", e)))?;
                                info!("tcp connect: TLS handshake complete");
                                UnifiedStream::ClientTls(tls_stream)
                            } else {
                                UnifiedStream::Plain(tcp_stream)
                            }
                        } else {
                            UnifiedStream::Plain(tcp_stream)
                        };

                        let id = st.next_id();
                        st.connections.lock().await.insert(
                            id,
                            ConnectionEntry {
                                stream: Arc::new(Mutex::new(StreamState::Full(Box::new(
                                    unified_stream,
                                )))),
                                peer_addr,
                                owner: actor_id,
                                state: ConnectionState::Active, // Outbound = active
                                data_mode: DataMode::Passive,
                            },
                        );
                        debug!("tcp connected to {} as conn={}", address, id);
                        Ok::<Value, Value>(Value::String(id_to_string(id)))
                    }
                },
            )?
            // ----------------------------------------------------------------
            // listen(address: string) -> result<string, string>
            // Binds a listener and spawns a background accept loop
            // ----------------------------------------------------------------
            .func_async_result(
                "listen",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let st = st_listen.clone();
                    let actor_id = aid_listen;
                    let actor_handle_arc = actor_handle_for_listen.clone();
                    let cancel_token = cancel_token_for_listen.clone();
                    let tls_ctx = tls_for_listen.clone();

                    async move {
                        let _ph = PhaseLog::new("tcp.listen");
                        let address = parse_string(&input)?;
                        debug!("tcp listen on {}", address);

                        let listener = TcpListener::bind(&address)
                            .await
                            .map_err(|e| Value::String(e.to_string()))?;

                        let listener_id = st.next_id();
                        let has_tls = match tls_ctx.as_ref() {
                            Some(ctx) => ctx.server_acceptor.is_some(),
                            None => false,
                        };
                        info!(
                            "tcp listening on {} as listener={} (tls={})",
                            address, listener_id, has_tls
                        );

                        // Take the actor_handle for use in the accept loop
                        let actor_handle = {
                            let guard = actor_handle_arc.lock().unwrap();
                            guard.clone()
                        };
                        let Some(actor_handle) = actor_handle else {
                            return Err(Value::String(
                                "Actor handle not available - setup() not called?".to_string(),
                            ));
                        };

                        // Clone state for the background task
                        let st_for_task = st.clone();
                        let actor_id_for_task = actor_id;

                        // Spawn background accept loop with cancellation support
                        tokio::spawn(async move {
                            info!("TCP accept loop started for listener={}", listener_id);

                            loop {
                                tokio::select! {
                                    _ = cancel_token.cancelled() => {
                                        info!("TCP accept loop cancelled for listener={}", listener_id);
                                        break;
                                    }
                                    result = listener.accept() => {
                                        match result {
                                            Ok((tcp_stream, peer_addr)) => {
                                                let conn_id = st_for_task.next_id();
                                                info!(
                                                    "tcp accepted conn={} from {} on listener={}",
                                                    conn_id, peer_addr, listener_id
                                                );

                                                // Apply TLS if configured
                                                let unified_stream = if let Some(ref ctx) = *tls_ctx {
                                                    if let Some(ref acceptor) = ctx.server_acceptor {
                                                        debug!("tcp accept: performing TLS handshake for conn={}", conn_id);
                                                        match acceptor.accept(tcp_stream).await {
                                                            Ok(tls_stream) => {
                                                                info!("tcp accept: TLS handshake complete for conn={}", conn_id);
                                                                UnifiedStream::ServerTls(tls_stream)
                                                            }
                                                            Err(e) => {
                                                                error!("TLS handshake failed for conn={}: {}", conn_id, e);
                                                                continue; // Skip this connection
                                                            }
                                                        }
                                                    } else {
                                                        UnifiedStream::Plain(tcp_stream)
                                                    }
                                                } else {
                                                    UnifiedStream::Plain(tcp_stream)
                                                };

                                                // Store connection in PENDING state
                                                st_for_task.connections.lock().await.insert(
                                                    conn_id,
                                                    ConnectionEntry {
                                                        stream: Arc::new(Mutex::new(
                                                            StreamState::Full(Box::new(unified_stream)),
                                                        )),
                                                        peer_addr,
                                                        owner: actor_id_for_task,
                                                        state: ConnectionState::Pending,
                                                        data_mode: DataMode::Passive,
                                                    },
                                                );

                                                // Detach so a slow/blocked handle-connection in the actor
                                                // cannot wedge the accept loop and saturate the kernel SYN queue.
                                                let conn_id_str = id_to_string(conn_id);
                                                let params =
                                                    Value::Tuple(vec![Value::String(conn_id_str)]);
                                                let actor_handle_for_call = actor_handle.clone();
                                                let st_for_cleanup = st_for_task.clone();
                                                tokio::spawn(async move {
                                                    if let Err(e) = actor_handle_for_call
                                                        .call_function(
                                                            "theater:simple/tcp-client.handle-connection"
                                                                .to_string(),
                                                            params,
                                                        )
                                                        .await
                                                    {
                                                        error!(
                                                            "Failed to call handle-connection for conn={}: {}",
                                                            conn_id, e
                                                        );
                                                        // Clean up the pending connection
                                                        st_for_cleanup
                                                            .connections
                                                            .lock()
                                                            .await
                                                            .remove(&conn_id);
                                                    }
                                                });
                                            }
                                            Err(e) => {
                                                error!(
                                                    "TCP accept error on listener={}: {}",
                                                    listener_id, e
                                                );
                                            }
                                        }
                                    }
                                }
                            }

                            info!("TCP accept loop stopped for listener={}", listener_id);
                        });

                        Ok::<Value, Value>(Value::String(id_to_string(listener_id)))
                    }
                },
            )?
            // ----------------------------------------------------------------
            // accept(listener-id: string) -> result<string, string>
            // Manual accept - returns connection in PENDING state
            // ----------------------------------------------------------------
            .func_async_result(
                "accept",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let st = st_accept.clone();
                    let actor_id = aid_accept;
                    let tls_ctx = tls_for_accept.clone();
                    async move {
                        let _ph = PhaseLog::new("tcp.accept");
                        let listener_id_str = parse_string(&input)?;
                        let listener_id = string_to_id(&listener_id_str)?;
                        debug!("tcp accept on listener={}", listener_id);

                        // Check ownership
                        let mut listeners = st.listeners.lock().await;
                        let entry = listeners.get_mut(&listener_id).ok_or_else(|| {
                            Value::String(format!("Listener not found: {}", listener_id_str))
                        })?;

                        if entry.owner != actor_id {
                            return Err(Value::String(format!(
                                "Listener {} not owned by this actor",
                                listener_id_str
                            )));
                        }

                        let (tcp_stream, peer_addr) = entry
                            .listener
                            .accept()
                            .await
                            .map_err(|e| Value::String(e.to_string()))?;

                        let conn_id = st.next_id();
                        drop(listeners); // Release lock before acquiring connections lock

                        // Apply TLS if configured
                        let unified_stream = if let Some(ref ctx) = *tls_ctx {
                            if let Some(ref acceptor) = ctx.server_acceptor {
                                debug!("tcp manual accept: performing TLS handshake for conn={}", conn_id);
                                let tls_stream = acceptor
                                    .accept(tcp_stream)
                                    .await
                                    .map_err(|e| Value::String(format!("TLS handshake failed: {}", e)))?;
                                info!("tcp manual accept: TLS handshake complete for conn={}", conn_id);
                                UnifiedStream::ServerTls(tls_stream)
                            } else {
                                UnifiedStream::Plain(tcp_stream)
                            }
                        } else {
                            UnifiedStream::Plain(tcp_stream)
                        };

                        st.connections.lock().await.insert(
                            conn_id,
                            ConnectionEntry {
                                stream: Arc::new(Mutex::new(StreamState::Full(Box::new(
                                    unified_stream,
                                )))),
                                peer_addr,
                                owner: actor_id,
                                state: ConnectionState::Pending, // Starts pending!
                                data_mode: DataMode::Passive,
                            },
                        );
                        debug!("tcp accepted conn={} from {} (pending)", conn_id, peer_addr);
                        Ok::<Value, Value>(Value::String(id_to_string(conn_id)))
                    }
                },
            )?
            // ----------------------------------------------------------------
            // activate(connection-id: string) -> result<_, string>
            // Activate a pending connection for this actor
            // ----------------------------------------------------------------
            .func_async_result(
                "activate",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let st = st_activate.clone();
                    let actor_id = aid_activate;
                    async move {
                        let _ph = PhaseLog::new("tcp.activate");
                        let conn_id_str = parse_string(&input)?;
                        let conn_id = string_to_id(&conn_id_str)?;

                        let mut connections = st.connections.lock().await;
                        let entry = connections.get_mut(&conn_id).ok_or_else(|| {
                            Value::String(format!("Connection not found: {}", conn_id_str))
                        })?;

                        if entry.owner != actor_id {
                            return Err(Value::String(format!(
                                "Connection {} not owned by this actor",
                                conn_id_str
                            )));
                        }

                        if entry.state == ConnectionState::Active {
                            return Err(Value::String(format!(
                                "Connection {} is already active",
                                conn_id_str
                            )));
                        }

                        entry.state = ConnectionState::Active;
                        debug!("tcp activated conn={}", conn_id);
                        Ok::<Value, Value>(Value::Tuple(vec![]))
                    }
                },
            )?
            // ----------------------------------------------------------------
            // set-active(connection-id: string, mode: string) -> result<_, string>
            // Set data mode: "passive", "active", or "once"
            // ----------------------------------------------------------------
            .func_async_result(
                "set-active",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let st = st_set_active.clone();
                    let actor_id = aid_set_active;
                    let actor_handle_arc = actor_handle_for_set_active.clone();
                    let cancel_token = cancel_token_for_set_active.clone();
                    async move {
                        let _ph = PhaseLog::new("tcp.set_active");
                        let (conn_id_str, mode_str) = parse_two_strings(&input)?;
                        let conn_id = string_to_id(&conn_id_str)?;

                        let new_mode = match mode_str.as_str() {
                            "passive" => DataMode::Passive,
                            "active" => DataMode::Active,
                            "once" => DataMode::Once,
                            _ => {
                                return Err(Value::String(format!(
                                    "Invalid mode '{}': expected 'passive', 'active', or 'once'",
                                    mode_str
                                )));
                            }
                        };

                        let mut connections = st.connections.lock().await;
                        let entry = connections.get_mut(&conn_id).ok_or_else(|| {
                            Value::String(format!("Connection not found: {}", conn_id_str))
                        })?;

                        if entry.owner != actor_id {
                            return Err(Value::String(format!(
                                "Connection {} not owned by this actor",
                                conn_id_str
                            )));
                        }

                        if entry.state != ConnectionState::Active {
                            return Err(Value::String(format!(
                                "Connection {} must be activated before setting data mode",
                                conn_id_str
                            )));
                        }

                        let old_mode = entry.data_mode;

                        // Handle mode transitions
                        match (old_mode, new_mode) {
                            (DataMode::Passive, DataMode::Active | DataMode::Once) => {
                                // Transitioning to active/once mode - split stream and spawn reader
                                let mut stream_guard = entry.stream.lock().await;
                                let stream = std::mem::replace(&mut *stream_guard, StreamState::Closed);
                                let full_stream = match stream {
                                    StreamState::Full(s) => s,
                                    other @ StreamState::WriteOnly(_) => {
                                        // Restore — we replaced with Closed above.
                                        *stream_guard = other;
                                        return Err(Value::String(format!(
                                            "Connection {} is already in active mode",
                                            conn_id_str
                                        )));
                                    }
                                    StreamState::Closed => {
                                        return Err(Value::String(format!(
                                            "Connection {} is closed",
                                            conn_id_str
                                        )));
                                    }
                                };

                                let (read_half, write_half) = full_stream.into_split();
                                *stream_guard = StreamState::WriteOnly(write_half);
                                drop(stream_guard);
                                entry.data_mode = new_mode;

                                // Get actor handle for callbacks
                                let actor_handle = {
                                    let guard = actor_handle_arc.lock().unwrap();
                                    guard.clone()
                                };
                                let Some(actor_handle) = actor_handle else {
                                    return Err(Value::String(
                                        "Actor handle not available".to_string(),
                                    ));
                                };

                                // Spawn background read task with cancellation support
                                let conn_id_for_task = conn_id;
                                let st_for_task = st.clone();
                                let is_once = new_mode == DataMode::Once;
                                let cancel_token_for_task = cancel_token.clone();

                                tokio::spawn(async move {
                                    tcp_read_loop(
                                        conn_id_for_task,
                                        read_half,
                                        actor_handle,
                                        st_for_task,
                                        is_once,
                                        cancel_token_for_task,
                                    )
                                    .await;
                                });

                                info!(
                                    "tcp conn={} set to {} mode, read loop spawned",
                                    conn_id, mode_str
                                );
                            }
                            (DataMode::Active | DataMode::Once, DataMode::Passive) => {
                                // Can't go back to passive once in active mode (stream is split)
                                return Err(Value::String(format!(
                                    "Cannot switch connection {} back to passive mode (stream is split)",
                                    conn_id_str
                                )));
                            }
                            (DataMode::Active, DataMode::Once) | (DataMode::Once, DataMode::Active) => {
                                // Can't switch between active and once (would need to stop/restart reader)
                                return Err(Value::String(format!(
                                    "Cannot switch connection {} between active and once modes",
                                    conn_id_str
                                )));
                            }
                            _ => {
                                // Same mode, no-op
                                debug!("tcp conn={} already in {} mode", conn_id, mode_str);
                            }
                        }

                        Ok::<Value, Value>(Value::Tuple(vec![]))
                    }
                },
            )?
            // ----------------------------------------------------------------
            // transfer(connection-id: string, target-actor: string) -> result<_, string>
            // Transfer connection to another actor (and activate it)
            // ----------------------------------------------------------------
            .func_async_result(
                "transfer",
                move |ctx: AsyncCtx<ActorStore>, input: Value| {
                    let st = st_transfer.clone();
                    let actor_id = aid_transfer;
                    async move {
                        let _ph = PhaseLog::new("tcp.transfer");
                        let (conn_id_str, target_actor_str) = parse_two_strings(&input)?;
                        let conn_id = string_to_id(&conn_id_str)?;

                        let target_actor: TheaterId = target_actor_str
                            .parse()
                            .map_err(|e| Value::String(format!("Invalid actor ID: {}", e)))?;

                        {
                            let mut connections = st.connections.lock().await;
                            let entry = connections.get_mut(&conn_id).ok_or_else(|| {
                                Value::String(format!("Connection not found: {}", conn_id_str))
                            })?;

                            if entry.owner != actor_id {
                                return Err(Value::String(format!(
                                    "Connection {} not owned by this actor",
                                    conn_id_str
                                )));
                            }

                            // Transfer ownership and activate
                            let old_owner = entry.owner;
                            entry.owner = target_actor;
                            entry.state = ConnectionState::Active;

                            info!(
                                "tcp transferred conn={} from {} to {} (now active)",
                                conn_id, old_owner, target_actor
                            );
                        }

                        // Get target actor's handle and call handle-connection-transfer
                        let store = ctx.data();
                        let theater_tx = store.theater_tx.clone();

                        let (handle_tx, handle_rx) = tokio::sync::oneshot::channel();
                        let get_handle_cmd = theater::messages::TheaterCommand::GetActorHandle {
                            actor_id: target_actor,
                            response_tx: handle_tx,
                        };
                        theater_tx.send(get_handle_cmd).await
                            .map_err(|e| Value::String(format!("Failed to get target handle: {}", e)))?;

                        let target_handle = match handle_rx.await {
                            Ok(Some(handle)) => handle,
                            Ok(None) => return Err(Value::String("Target actor handle not found".to_string())),
                            Err(e) => return Err(Value::String(format!("Failed to receive handle: {}", e))),
                        };

                        // Call handle-connection-transfer on target
                        // Just pass conn_id - runtime will prepend state to make (state, conn_id)
                        let params = Value::String(conn_id_str.clone());
                        if let Err(e) = target_handle
                            .call_function(
                                "theater:simple/tcp-client.handle-connection-transfer".to_string(),
                                params,
                            )
                            .await
                        {
                            warn!("Failed to call handle-connection-transfer: {:?}", e);
                            // Don't fail the transfer, just log the warning
                        }

                        Ok::<Value, Value>(Value::Tuple(vec![]))
                    }
                },
            )?
            // ----------------------------------------------------------------
            // peer-address(connection-id: string) -> result<string, string>
            // Get peer address (works in pending or active state)
            // ----------------------------------------------------------------
            .func_async_result(
                "peer-address",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let st = st_peer.clone();
                    let actor_id = aid_peer;
                    async move {
                        let _ph = PhaseLog::new("tcp.peer_address");
                        let conn_id_str = parse_string(&input)?;
                        let conn_id = string_to_id(&conn_id_str)?;

                        let connections = st.connections.lock().await;
                        let entry = connections.get(&conn_id).ok_or_else(|| {
                            Value::String(format!("Connection not found: {}", conn_id_str))
                        })?;

                        if entry.owner != actor_id {
                            return Err(Value::String(format!(
                                "Connection {} not owned by this actor",
                                conn_id_str
                            )));
                        }

                        Ok::<Value, Value>(Value::String(entry.peer_addr.to_string()))
                    }
                },
            )?
            // ----------------------------------------------------------------
            // send(connection-id: string, data: list<u8>) -> result<u64, string>
            // ----------------------------------------------------------------
            .func_async_result(
                "send",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let st = st_send.clone();
                    let actor_id = aid_send;
                    async move {
                        let _ph = PhaseLog::new("tcp.send");
                        let (conn_id_str, data) = parse_string_and_bytes(&input)?;
                        let conn_id = string_to_id(&conn_id_str)?;
                        let len = data.len();

                        // Lock the outer map only long enough to validate metadata
                        // and clone the per-connection stream Arc. The actual I/O
                        // runs without holding the outer lock — that lets two
                        // actors do I/O on different connections in parallel.
                        let stream_arc = {
                            let connections = st.connections.lock().await;
                            let entry = connections.get(&conn_id).ok_or_else(|| {
                                Value::String(format!("Connection not found: {}", conn_id_str))
                            })?;

                            if entry.owner != actor_id {
                                return Err(Value::String(format!(
                                    "Connection {} not owned by this actor",
                                    conn_id_str
                                )));
                            }

                            if entry.state == ConnectionState::Pending {
                                return Err(Value::String(format!(
                                    "Connection {} is pending - call activate() or transfer() first",
                                    conn_id_str
                                )));
                            }

                            entry.stream.clone()
                        };

                        let mut stream_guard = stream_arc.lock().await;
                        match &mut *stream_guard {
                            StreamState::Full(stream) => {
                                stream
                                    .write_all(&data)
                                    .await
                                    .map_err(|e| Value::String(e.to_string()))?;
                            }
                            StreamState::WriteOnly(write_half) => {
                                write_half
                                    .write_all(&data)
                                    .await
                                    .map_err(|e| Value::String(e.to_string()))?;
                            }
                            StreamState::Closed => {
                                return Err(Value::String(format!(
                                    "Connection {} is closed",
                                    conn_id_str
                                )));
                            }
                        }

                        debug!("tcp send conn={} {} bytes", conn_id, len);
                        Ok::<Value, Value>(Value::U64(len as u64))
                    }
                },
            )?
            // ----------------------------------------------------------------
            // receive(connection-id: string, max-bytes: u32) -> result<list<u8>, string>
            // ----------------------------------------------------------------
            .func_async_result(
                "receive",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let st = st_receive.clone();
                    let actor_id = aid_receive;
                    let cancel_token = cancel_token_for_receive.clone();
                    async move {
                        let _ph = PhaseLog::new("tcp.receive");
                        let (conn_id_str, max_bytes) = parse_string_and_u32(&input)?;
                        let conn_id = string_to_id(&conn_id_str)?;

                        // Lock the outer map only long enough to validate metadata
                        // and clone the per-connection stream Arc. The actual read
                        // runs without holding the outer lock so other actors can
                        // do their own I/O concurrently on other connections.
                        let stream_arc = {
                            let connections = st.connections.lock().await;
                            let entry = connections.get(&conn_id).ok_or_else(|| {
                                Value::String(format!("Connection not found: {}", conn_id_str))
                            })?;

                            if entry.owner != actor_id {
                                return Err(Value::String(format!(
                                    "Connection {} not owned by this actor",
                                    conn_id_str
                                )));
                            }

                            if entry.state == ConnectionState::Pending {
                                return Err(Value::String(format!(
                                    "Connection {} is pending - call activate() or transfer() first",
                                    conn_id_str
                                )));
                            }

                            if entry.data_mode != DataMode::Passive {
                                return Err(Value::String(format!(
                                    "Connection {} is in active mode - data is pushed via on-data callback",
                                    conn_id_str
                                )));
                            }

                            entry.stream.clone()
                        };

                        let mut stream_guard = stream_arc.lock().await;
                        let stream = match &mut *stream_guard {
                            StreamState::Full(stream) => stream,
                            StreamState::WriteOnly(_) => {
                                return Err(Value::String(format!(
                                    "Connection {} read half not available (in active mode)",
                                    conn_id_str
                                )));
                            }
                            StreamState::Closed => {
                                return Err(Value::String(format!(
                                    "Connection {} is closed",
                                    conn_id_str
                                )));
                            }
                        };

                        let mut buf = vec![0u8; max_bytes as usize];

                        // Use select to make the read interruptible on shutdown
                        let n = tokio::select! {
                            result = stream.read(&mut buf) => {
                                result.map_err(|e| Value::String(e.to_string()))?
                            }
                            _ = cancel_token.cancelled() => {
                                info!("TCP receive cancelled due to shutdown, conn={}", conn_id);
                                return Err(Value::String("Connection closed: actor shutting down".to_string()));
                            }
                        };

                        debug!(
                            "tcp receive conn={} {} bytes (max={})",
                            conn_id, n, max_bytes
                        );
                        buf.truncate(n);
                        Ok::<Value, Value>(Value::List {
                            elem_type: ValueType::U8,
                            items: buf.into_iter().map(Value::U8).collect(),
                        })
                    }
                },
            )?
            // ----------------------------------------------------------------
            // close(connection-id: string) -> result<_, string>
            //
            // Gracefully shuts the write side of the stream before dropping —
            // for TLS streams this sends the close_notify alert so strict
            // clients (e.g. rustls) don't see an "unexpected EOF". Plain TCP
            // streams get a normal FIN.
            // ----------------------------------------------------------------
            .func_async_result(
                "close",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let st = st_close.clone();
                    let actor_id = aid_close;
                    async move {
                        let _ph = PhaseLog::new("tcp.close");
                        let conn_id_str = parse_string(&input)?;
                        let conn_id = string_to_id(&conn_id_str)?;

                        // Take the stream out of the map first, holding the
                        // outer lock only long enough to validate ownership
                        // and remove the entry.
                        let stream_arc = {
                            let mut connections = st.connections.lock().await;
                            let entry = connections.get(&conn_id).ok_or_else(|| {
                                Value::String(format!("Connection not found: {}", conn_id_str))
                            })?;
                            if entry.owner != actor_id {
                                return Err(Value::String(format!(
                                    "Connection {} not owned by this actor",
                                    conn_id_str
                                )));
                            }
                            let arc = entry.stream.clone();
                            connections.remove(&conn_id);
                            arc
                        };

                        // Move the stream out and call shutdown on the write
                        // side. Errors are non-fatal — peer may already have
                        // closed — but we log them at WARN so production can
                        // tell apart a clean close_notify-sent path from a
                        // shutdown failure that drops bytes (ticket #10).
                        let mut guard = stream_arc.lock().await;
                        let taken = std::mem::replace(&mut *guard, StreamState::Closed);
                        drop(guard);
                        let shutdown_result = match taken {
                            StreamState::Full(mut s) => {
                                Some(AsyncWriteExt::shutdown(&mut *s).await)
                            }
                            StreamState::WriteOnly(mut w) => {
                                Some(AsyncWriteExt::shutdown(&mut w).await)
                            }
                            StreamState::Closed => None,
                        };
                        match shutdown_result {
                            Some(Ok(())) => {
                                debug!("tcp close conn={} (graceful shutdown ok)", conn_id);
                            }
                            Some(Err(e)) => {
                                warn!(
                                    "tcp close conn={} shutdown error (close_notify may not have been sent): {}",
                                    conn_id, e
                                );
                            }
                            None => {
                                debug!("tcp close conn={} (already closed)", conn_id);
                            }
                        }
                        Ok::<Value, Value>(Value::Tuple(vec![]))
                    }
                },
            )?
            // ----------------------------------------------------------------
            // upgrade-to-tls-server(connection-id: string) -> result<_, string>
            //
            // For STARTTLS-style protocols: the actor accepts a plain TCP
            // connection, exchanges a few protocol lines, then calls this to
            // wrap the existing stream with TLS using the server_tls cert
            // configured on this handler. After this returns Ok, the same
            // connection-id transports TLS-encrypted bytes.
            // ----------------------------------------------------------------
            .func_async_result(
                "upgrade-to-tls-server",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let st = st_upgrade_server.clone();
                    let actor_id = aid_upgrade_server;
                    let tls_ctx = tls_for_upgrade_server.clone();
                    async move {
                        let _ph = PhaseLog::new("tcp.upgrade_to_tls_server");
                        let conn_id_str = parse_string(&input)?;
                        let conn_id = string_to_id(&conn_id_str)?;

                        let acceptor = match tls_ctx.as_ref() {
                            Some(ctx) => match &ctx.server_acceptor {
                                Some(a) => a.clone(),
                                None => {
                                    return Err(Value::String(
                                        "server_tls not configured on this handler".into(),
                                    ))
                                }
                            },
                            None => {
                                return Err(Value::String(
                                    "server_tls not configured on this handler".into(),
                                ))
                            }
                        };

                        let stream_arc = {
                            let connections = st.connections.lock().await;
                            let entry = connections.get(&conn_id).ok_or_else(|| {
                                Value::String(format!("Connection not found: {}", conn_id_str))
                            })?;
                            if entry.owner != actor_id {
                                return Err(Value::String(format!(
                                    "Connection {} not owned by this actor",
                                    conn_id_str
                                )));
                            }
                            if entry.state != ConnectionState::Active {
                                return Err(Value::String(format!(
                                    "Connection {} must be activated before TLS upgrade",
                                    conn_id_str
                                )));
                            }
                            if entry.data_mode != DataMode::Passive {
                                return Err(Value::String(format!(
                                    "Connection {} must be in passive mode for TLS upgrade",
                                    conn_id_str
                                )));
                            }
                            entry.stream.clone()
                        };

                        let mut guard = stream_arc.lock().await;
                        let taken = std::mem::replace(&mut *guard, StreamState::Closed);
                        let inner = match taken {
                            StreamState::Full(boxed) => *boxed,
                            StreamState::WriteOnly(w) => {
                                *guard = StreamState::WriteOnly(w);
                                return Err(Value::String(format!(
                                    "Connection {} is split; TLS upgrade not supported",
                                    conn_id_str
                                )));
                            }
                            StreamState::Closed => {
                                return Err(Value::String(format!(
                                    "Connection {} is closed",
                                    conn_id_str
                                )));
                            }
                        };
                        let tcp = match inner {
                            UnifiedStream::Plain(tcp) => tcp,
                            other => {
                                *guard = StreamState::Full(Box::new(other));
                                return Err(Value::String(format!(
                                    "Connection {} is already TLS",
                                    conn_id_str
                                )));
                            }
                        };

                        let tls_stream = match acceptor.accept(tcp).await {
                            Ok(s) => s,
                            Err(e) => {
                                // Stream is gone — leave entry as Closed.
                                return Err(Value::String(format!(
                                    "TLS server handshake failed: {}",
                                    e
                                )));
                            }
                        };
                        *guard = StreamState::Full(Box::new(UnifiedStream::ServerTls(tls_stream)));
                        drop(guard);

                        debug!("tcp upgrade-to-tls-server conn={}", conn_id);
                        Ok::<Value, Value>(Value::Tuple(vec![]))
                    }
                },
            )?
            // ----------------------------------------------------------------
            // upgrade-to-tls-client(connection-id, server-name) -> result<_, string>
            //
            // The client-side mirror of upgrade-to-tls-server: wraps an
            // existing plain TCP connection with TLS using the client_tls
            // config. server-name is used for SNI and cert verification.
            // ----------------------------------------------------------------
            .func_async_result(
                "upgrade-to-tls-client",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let st = st_upgrade_client.clone();
                    let actor_id = aid_upgrade_client;
                    let tls_ctx = tls_for_upgrade_client.clone();
                    async move {
                        let _ph = PhaseLog::new("tcp.upgrade_to_tls_client");
                        let (conn_id_str, server_name_str) = parse_two_strings(&input)?;
                        let conn_id = string_to_id(&conn_id_str)?;

                        let connector = match tls_ctx.as_ref() {
                            Some(ctx) => match &ctx.client_connector {
                                Some(c) => c.clone(),
                                None => {
                                    return Err(Value::String(
                                        "client_tls not configured on this handler".into(),
                                    ))
                                }
                            },
                            None => {
                                return Err(Value::String(
                                    "client_tls not configured on this handler".into(),
                                ))
                            }
                        };

                        let server_name =
                            rustls::pki_types::ServerName::try_from(server_name_str.clone())
                                .map_err(|e| {
                                    Value::String(format!(
                                        "Invalid server name {:?}: {}",
                                        server_name_str, e
                                    ))
                                })?;

                        let stream_arc = {
                            let connections = st.connections.lock().await;
                            let entry = connections.get(&conn_id).ok_or_else(|| {
                                Value::String(format!("Connection not found: {}", conn_id_str))
                            })?;
                            if entry.owner != actor_id {
                                return Err(Value::String(format!(
                                    "Connection {} not owned by this actor",
                                    conn_id_str
                                )));
                            }
                            if entry.state != ConnectionState::Active {
                                return Err(Value::String(format!(
                                    "Connection {} must be activated before TLS upgrade",
                                    conn_id_str
                                )));
                            }
                            if entry.data_mode != DataMode::Passive {
                                return Err(Value::String(format!(
                                    "Connection {} must be in passive mode for TLS upgrade",
                                    conn_id_str
                                )));
                            }
                            entry.stream.clone()
                        };

                        let mut guard = stream_arc.lock().await;
                        let taken = std::mem::replace(&mut *guard, StreamState::Closed);
                        let inner = match taken {
                            StreamState::Full(boxed) => *boxed,
                            StreamState::WriteOnly(w) => {
                                *guard = StreamState::WriteOnly(w);
                                return Err(Value::String(format!(
                                    "Connection {} is split; TLS upgrade not supported",
                                    conn_id_str
                                )));
                            }
                            StreamState::Closed => {
                                return Err(Value::String(format!(
                                    "Connection {} is closed",
                                    conn_id_str
                                )));
                            }
                        };
                        let tcp = match inner {
                            UnifiedStream::Plain(tcp) => tcp,
                            other => {
                                *guard = StreamState::Full(Box::new(other));
                                return Err(Value::String(format!(
                                    "Connection {} is already TLS",
                                    conn_id_str
                                )));
                            }
                        };

                        let tls_stream = match connector.connect(server_name, tcp).await {
                            Ok(s) => s,
                            Err(e) => {
                                return Err(Value::String(format!(
                                    "TLS client handshake failed: {}",
                                    e
                                )));
                            }
                        };
                        *guard = StreamState::Full(Box::new(UnifiedStream::ClientTls(tls_stream)));
                        drop(guard);

                        debug!(
                            "tcp upgrade-to-tls-client conn={} server_name={}",
                            conn_id, server_name_str
                        );
                        Ok::<Value, Value>(Value::Tuple(vec![]))
                    }
                },
            )?
            // ----------------------------------------------------------------
            // close-listener(listener-id: string) -> result<_, string>
            // ----------------------------------------------------------------
            .func_async_result(
                "close-listener",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let st = st_close_listener.clone();
                    let actor_id = aid_close_listener;
                    async move {
                        let _ph = PhaseLog::new("tcp.close_listener");
                        let listener_id_str = parse_string(&input)?;
                        let listener_id = string_to_id(&listener_id_str)?;

                        let mut listeners = st.listeners.lock().await;
                        let entry = listeners.get(&listener_id).ok_or_else(|| {
                            Value::String(format!("Listener not found: {}", listener_id_str))
                        })?;

                        if entry.owner != actor_id {
                            return Err(Value::String(format!(
                                "Listener {} not owned by this actor",
                                listener_id_str
                            )));
                        }

                        listeners.remove(&listener_id);
                        debug!("tcp close listener={}", listener_id);
                        Ok::<Value, Value>(Value::Tuple(vec![]))
                    }
                },
            )?;

        ctx.mark_satisfied("theater:simple/tcp");
        info!("TCP host functions (Pack) set up successfully");
        Ok(())
    }

    fn supports_composite(&self) -> bool {
        true
    }
}

// ============================================================================
// Active Mode Read Loop
// ============================================================================

/// Buffer size for active mode reads
const ACTIVE_READ_BUFFER_SIZE: usize = 8192;

/// Remove the connection entry from the shared map and, for TLS streams in
/// the `WriteOnly(write_half)` state, flush the close_notify alert before the
/// underlying TCP gets FIN'd.
///
/// Used by `tcp_read_loop` on every cleanup branch (peer-EOF, read error,
/// cancellation). Without the shutdown call, dropping the WriteOnly write
/// half drops the rustls session without flushing its outgoing close_notify
/// alert — peers then observe a bare TCP FIN and surface it as
/// `UnexpectedEof` ("peer closed connection without sending TLS
/// close_notify"). The explicit `close` host function had this right
/// already (PR #45); this is the parallel fix for the auto-cleanup path.
async fn shutdown_write_half_and_remove(shared_state: &Arc<SharedTcpState>, conn_id: u64) {
    let stream_arc = {
        let mut connections = shared_state.connections.lock().await;
        connections.remove(&conn_id).map(|e| e.stream)
    };
    let Some(stream_arc) = stream_arc else {
        return;
    };
    let mut guard = stream_arc.lock().await;
    let taken = std::mem::replace(&mut *guard, StreamState::Closed);
    drop(guard);
    match taken {
        StreamState::WriteOnly(mut w) => {
            if let Err(e) = AsyncWriteExt::shutdown(&mut w).await {
                warn!(
                    "tcp conn={} cleanup shutdown error (close_notify may not have been sent): {}",
                    conn_id, e
                );
            }
        }
        StreamState::Full(mut s) => {
            // Active-mode normally leaves the entry in WriteOnly state, but
            // handle Full defensively in case the reader exits before set-active
            // could complete the split.
            if let Err(e) = AsyncWriteExt::shutdown(&mut *s).await {
                warn!(
                    "tcp conn={} cleanup shutdown error (close_notify may not have been sent): {}",
                    conn_id, e
                );
            }
        }
        StreamState::Closed => {}
    }
}

/// Background task that reads from a connection and calls on-data/on-close callbacks.
///
/// This is spawned when a connection enters "active" or "once" mode.
async fn tcp_read_loop(
    conn_id: u64,
    mut read_half: UnifiedReadHalf,
    actor_handle: ActorHandle,
    shared_state: Arc<SharedTcpState>,
    is_once: bool,
    cancel_token: CancellationToken,
) {
    let conn_id_str = id_to_string(conn_id);
    info!(
        "tcp read loop started for conn={} (once={})",
        conn_id, is_once
    );

    let mut buf = vec![0u8; ACTIVE_READ_BUFFER_SIZE];

    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                info!("tcp read loop cancelled for conn={}", conn_id);
                shutdown_write_half_and_remove(&shared_state, conn_id).await;
                break;
            }
            result = read_half.read(&mut buf) => {
                match result {
                    Ok(0) => {
                        // EOF - connection closed by peer
                        info!("tcp conn={} received EOF", conn_id);

                        // Call on-close callback
                        let params = Value::Tuple(vec![
                            Value::String(conn_id_str.clone()),
                            Value::String("eof".to_string()),
                        ]);

                        if let Err(e) = actor_handle
                            .call_function("theater:simple/tcp-client.on-close".to_string(), params)
                            .await
                        {
                            warn!("tcp conn={} on-close callback failed: {}", conn_id, e);
                        }

                        shutdown_write_half_and_remove(&shared_state, conn_id).await;
                        break;
                    }
                    Ok(n) => {
                        // Data received - call on-data callback
                        let data = buf[..n].to_vec();
                        debug!("tcp conn={} received {} bytes, calling on-data", conn_id, n);

                        let params = Value::Tuple(vec![
                            Value::String(conn_id_str.clone()),
                            Value::List {
                                elem_type: ValueType::U8,
                                items: data.into_iter().map(Value::U8).collect(),
                            },
                        ]);

                        if let Err(e) = actor_handle
                            .call_function("theater:simple/tcp-client.on-data".to_string(), params)
                            .await
                        {
                            error!("tcp conn={} on-data callback failed: {}", conn_id, e);
                            // Continue reading even if callback fails
                        }

                        if is_once {
                            // Once mode: switch back to passive after one read
                            info!("tcp conn={} once mode complete, switching to passive", conn_id);

                            // Update the connection's data mode
                            if let Some(entry) = shared_state.connections.lock().await.get_mut(&conn_id) {
                                entry.data_mode = DataMode::Passive;
                            }
                            break;
                        }
                    }
                    Err(e) => {
                        // Read error - connection broken
                        error!("tcp conn={} read error: {}", conn_id, e);

                        // Call on-close callback with error
                        let params = Value::Tuple(vec![
                            Value::String(conn_id_str.clone()),
                            Value::String(e.to_string()),
                        ]);

                        if let Err(e) = actor_handle
                            .call_function("theater:simple/tcp-client.on-close".to_string(), params)
                            .await
                        {
                            warn!("tcp conn={} on-close callback failed: {}", conn_id, e);
                        }

                        shutdown_write_half_and_remove(&shared_state, conn_id).await;
                        break;
                    }
                }
            }
        }
    }

    info!("tcp read loop stopped for conn={}", conn_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tcp_handler_creation() {
        let config = TcpHandlerConfig::default();
        let handler = TcpHandler::new(config);

        assert_eq!(handler.name(), "tcp");
        assert_eq!(
            handler.imports(),
            Some(vec!["theater:simple/tcp".to_string()])
        );
        assert_eq!(
            handler.exports(),
            Some(vec!["theater:simple/tcp-client".to_string()])
        );
    }

    #[test]
    fn test_tcp_handler_clone_shares_state() {
        let config = TcpHandlerConfig::default();
        let handler = TcpHandler::new(config);
        let cloned = handler.create_instance(None);

        // Both should have the same name
        assert_eq!(cloned.name(), "tcp");

        // The key test: shared_state Arc should be the same
        // (We can't easily test this without exposing internals, but the
        // implementation clones the Arc, not the data)
    }

    #[test]
    fn test_tcp_interface_hash_determinism() {
        let interface1 = tcp_interface();
        let interface2 = tcp_interface();
        assert_eq!(interface1.hash(), interface2.hash());
    }

    #[test]
    fn test_tcp_handler_interface_hashes() {
        let config = TcpHandlerConfig::default();
        let handler = TcpHandler::new(config);

        let hashes = handler.interface_hashes();
        assert_eq!(hashes.len(), 1);
        assert_eq!(hashes[0].0, "theater:simple/tcp");

        // Hash should be non-zero
        assert!(!hashes[0].1.as_bytes().iter().all(|&b| b == 0));
    }

    #[test]
    fn test_connection_state_enum() {
        assert_ne!(ConnectionState::Pending, ConnectionState::Active);
    }

    #[test]
    fn test_data_mode_enum() {
        assert_ne!(DataMode::Passive, DataMode::Active);
        assert_ne!(DataMode::Passive, DataMode::Once);
        assert_ne!(DataMode::Active, DataMode::Once);
    }
}

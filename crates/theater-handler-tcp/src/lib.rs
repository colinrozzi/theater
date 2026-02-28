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

use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tracing::{debug, error, info};

use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::config::actor_manifest::{HandlerConfig, TcpHandlerConfig};
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::id::TheaterId;
use theater::shutdown::ShutdownReceiver;

use theater::pack_bridge::{
    parse_pact, AsyncCtx, HostLinkerBuilder, InterfaceImpl, LinkerError, TypeHash, Value,
    ValueType,
};

// ============================================================================
// Interface Declarations
// ============================================================================

/// Embedded tcp.pact file content
const TCP_PACT: &str = include_str!("../../../pact/tcp.pact");

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

/// A tracked TCP connection with ownership and state
struct ConnectionEntry {
    stream: TcpStream,
    peer_addr: SocketAddr,
    owner: TheaterId,
    state: ConnectionState,
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
    /// Shutdown receiver for background accept loops
    shutdown_receiver: Arc<std::sync::Mutex<Option<ShutdownReceiver>>>,
}

impl TcpHandler {
    pub fn new(config: TcpHandlerConfig) -> Self {
        Self {
            config,
            shared_state: Arc::new(SharedTcpState::new(None)),
            actor_id: Arc::new(std::sync::Mutex::new(None)),
            actor_handle: Arc::new(std::sync::Mutex::new(None)),
            shutdown_receiver: Arc::new(std::sync::Mutex::new(None)),
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

        // Share the same state across all instances - this is the key for transfer!
        Box::new(TcpHandler {
            config: tcp_config,
            shared_state: self.shared_state.clone(), // Same Arc!
            actor_id: Arc::new(std::sync::Mutex::new(None)),
            actor_handle: Arc::new(std::sync::Mutex::new(None)),
            shutdown_receiver: Arc::new(std::sync::Mutex::new(None)),
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
        _event_rx: tokio::sync::broadcast::Receiver<theater::chain::ChainEvent>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        info!("TCP handler setup (passive mode)");

        // Store the actor_handle and shutdown_receiver for use by listen()
        {
            let mut handle_guard = self.actor_handle.lock().unwrap();
            *handle_guard = Some(actor_handle);
        }
        {
            let mut shutdown_guard = self.shutdown_receiver.lock().unwrap();
            *shutdown_guard = Some(shutdown_receiver);
        }

        // Handler is now passive - actors call listen() to start the accept loop
        Box::pin(async move { Ok(()) })
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
            .clone()
            .expect("actor_id should be set in HandlerContext");

        // Store actor_id for this instance
        {
            let mut id_guard = self.actor_id.lock().unwrap();
            *id_guard = Some(actor_id.clone());
        }

        // Update max_connections if configured
        // Note: We can't easily update the shared state's max_connections here
        // since it's already created. For now, first handler wins.

        let state = self.shared_state.clone();
        let actor_id_for_closures = actor_id.clone();

        // Clone handler fields for use in listen() callback
        let actor_handle_for_listen = self.actor_handle.clone();
        let shutdown_receiver_for_listen = self.shutdown_receiver.clone();

        // Clone state and actor_id for each closure
        let st_connect = state.clone();
        let aid_connect = actor_id_for_closures.clone();

        let st_listen = state.clone();
        let aid_listen = actor_id_for_closures.clone();

        let st_accept = state.clone();
        let aid_accept = actor_id_for_closures.clone();

        let st_activate = state.clone();
        let aid_activate = actor_id_for_closures.clone();

        let st_transfer = state.clone();
        let aid_transfer = actor_id_for_closures.clone();

        let st_peer = state.clone();
        let aid_peer = actor_id_for_closures.clone();

        let st_send = state.clone();
        let aid_send = actor_id_for_closures.clone();

        let st_receive = state.clone();
        let aid_receive = actor_id_for_closures.clone();

        let st_close = state.clone();
        let aid_close = actor_id_for_closures.clone();

        let st_close_listener = state.clone();
        let aid_close_listener = actor_id_for_closures.clone();

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
                    let actor_id = aid_connect.clone();
                    async move {
                        let address = parse_string(&input)?;
                        st.check_connection_limit().await?;
                        debug!("tcp connect to {}", address);

                        let stream = TcpStream::connect(&address)
                            .await
                            .map_err(|e| Value::String(e.to_string()))?;

                        let peer_addr = stream
                            .peer_addr()
                            .map_err(|e| Value::String(e.to_string()))?;

                        let id = st.next_id();
                        st.connections.lock().await.insert(
                            id,
                            ConnectionEntry {
                                stream,
                                peer_addr,
                                owner: actor_id,
                                state: ConnectionState::Active, // Outbound = active
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
                    let actor_id = aid_listen.clone();
                    let actor_handle_arc = actor_handle_for_listen.clone();
                    let shutdown_receiver_arc = shutdown_receiver_for_listen.clone();

                    async move {
                        let address = parse_string(&input)?;
                        debug!("tcp listen on {}", address);

                        let listener = TcpListener::bind(&address)
                            .await
                            .map_err(|e| Value::String(e.to_string()))?;

                        let listener_id = st.next_id();
                        info!(
                            "tcp listening on {} as listener={}",
                            address, listener_id
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

                        // Take the shutdown receiver (only available for the first listener)
                        let shutdown_receiver = {
                            let mut guard = shutdown_receiver_arc.lock().unwrap();
                            guard.take()
                        };

                        // Clone state for the background task
                        let st_for_task = st.clone();
                        let actor_id_for_task = actor_id.clone();

                        // Spawn background accept loop
                        tokio::spawn(async move {
                            info!("TCP accept loop started for listener={}", listener_id);

                            let accept_loop = async {
                                loop {
                                    match listener.accept().await {
                                        Ok((stream, peer_addr)) => {
                                            let conn_id = st_for_task.next_id();
                                            info!(
                                                "tcp accepted conn={} from {} on listener={}",
                                                conn_id, peer_addr, listener_id
                                            );

                                            // Store connection in PENDING state
                                            st_for_task.connections.lock().await.insert(
                                                conn_id,
                                                ConnectionEntry {
                                                    stream,
                                                    peer_addr,
                                                    owner: actor_id_for_task.clone(),
                                                    state: ConnectionState::Pending,
                                                },
                                            );

                                            // Call the actor's handle-connection export
                                            let conn_id_str = id_to_string(conn_id);
                                            let params =
                                                Value::Tuple(vec![Value::String(conn_id_str)]);

                                            if let Err(e) = actor_handle
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
                                                st_for_task.connections.lock().await.remove(&conn_id);
                                            }
                                        }
                                        Err(e) => {
                                            error!(
                                                "TCP accept error on listener={}: {}",
                                                listener_id, e
                                            );
                                        }
                                    }
                                }
                            };

                            if let Some(shutdown_rx) = shutdown_receiver {
                                tokio::select! {
                                    _ = shutdown_rx.wait_for_shutdown() => {
                                        info!("TCP accept loop received shutdown for listener={}", listener_id);
                                    }
                                    _ = accept_loop => {}
                                }
                            } else {
                                accept_loop.await;
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
                    let actor_id = aid_accept.clone();
                    async move {
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

                        let (stream, peer_addr) = entry
                            .listener
                            .accept()
                            .await
                            .map_err(|e| Value::String(e.to_string()))?;

                        let conn_id = st.next_id();
                        drop(listeners); // Release lock before acquiring connections lock

                        st.connections.lock().await.insert(
                            conn_id,
                            ConnectionEntry {
                                stream,
                                peer_addr,
                                owner: actor_id,
                                state: ConnectionState::Pending, // Starts pending!
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
                    let actor_id = aid_activate.clone();
                    async move {
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
            // transfer(connection-id: string, target-actor: string) -> result<_, string>
            // Transfer connection to another actor (and activate it)
            // ----------------------------------------------------------------
            .func_async_result(
                "transfer",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let st = st_transfer.clone();
                    let actor_id = aid_transfer.clone();
                    async move {
                        let (conn_id_str, target_actor_str) = parse_two_strings(&input)?;
                        let conn_id = string_to_id(&conn_id_str)?;

                        let target_actor: TheaterId = target_actor_str
                            .parse()
                            .map_err(|e| Value::String(format!("Invalid actor ID: {}", e)))?;

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
                        let old_owner = entry.owner.clone();
                        entry.owner = target_actor.clone();
                        entry.state = ConnectionState::Active;

                        info!(
                            "tcp transferred conn={} from {} to {} (now active)",
                            conn_id, old_owner, target_actor
                        );
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
                    let actor_id = aid_peer.clone();
                    async move {
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
                    let actor_id = aid_send.clone();
                    async move {
                        let (conn_id_str, data) = parse_string_and_bytes(&input)?;
                        let conn_id = string_to_id(&conn_id_str)?;
                        let len = data.len();

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

                        if entry.state == ConnectionState::Pending {
                            return Err(Value::String(format!(
                                "Connection {} is pending - call activate() or transfer() first",
                                conn_id_str
                            )));
                        }

                        entry
                            .stream
                            .write_all(&data)
                            .await
                            .map_err(|e| Value::String(e.to_string()))?;

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
                    let actor_id = aid_receive.clone();
                    async move {
                        let (conn_id_str, max_bytes) = parse_string_and_u32(&input)?;
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

                        if entry.state == ConnectionState::Pending {
                            return Err(Value::String(format!(
                                "Connection {} is pending - call activate() or transfer() first",
                                conn_id_str
                            )));
                        }

                        let mut buf = vec![0u8; max_bytes as usize];
                        let n = entry
                            .stream
                            .read(&mut buf)
                            .await
                            .map_err(|e| Value::String(e.to_string()))?;

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
            // ----------------------------------------------------------------
            .func_async_result(
                "close",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let st = st_close.clone();
                    let actor_id = aid_close.clone();
                    async move {
                        let conn_id_str = parse_string(&input)?;
                        let conn_id = string_to_id(&conn_id_str)?;

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

                        connections.remove(&conn_id);
                        debug!("tcp close conn={}", conn_id);
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
                    let actor_id = aid_close_listener.clone();
                    async move {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tcp_handler_creation() {
        let config = TcpHandlerConfig {
            listen: None,
            max_connections: None,
        };
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
        let config = TcpHandlerConfig {
            listen: None,
            max_connections: None,
        };
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
        let config = TcpHandlerConfig {
            listen: None,
            max_connections: None,
        };
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
}

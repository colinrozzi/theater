//! # TCP Handler
//!
//! Provides raw TCP networking capabilities to WebAssembly actors in the Theater system.
//! This handler is deliberately minimal — it moves bytes across the boundary and leaves
//! all protocol complexity (framing, routing, addressing) to actor-space code.

use std::collections::HashMap;
use std::future::Future;
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
use theater::shutdown::ShutdownReceiver;

use theater::pack_bridge::{
    AsyncCtx, HostLinkerBuilder, LinkerError, PackInstance, Value, ValueType,
};

/// Shared TCP state between host function closures and the start() accept loop.
#[derive(Clone)]
struct TcpState {
    connections: Arc<Mutex<HashMap<u64, TcpStream>>>,
    listeners: Arc<Mutex<HashMap<u64, TcpListener>>>,
    next_id: Arc<AtomicU64>,
    max_connections: Option<u32>,
}

impl TcpState {
    fn new(max_connections: Option<u32>) -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
            listeners: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(AtomicU64::new(1)),
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

/// Handler for providing raw TCP networking access to WebAssembly actors.
#[derive(Clone)]
pub struct TcpHandler {
    config: TcpHandlerConfig,
    state: Option<TcpState>,
}

impl TcpHandler {
    pub fn new(config: TcpHandlerConfig) -> Self {
        Self {
            config,
            state: None,
        }
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
        _ => Err(Value::String(
            "Expected tuple (id, data)".to_string(),
        )),
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
        _ => Err(Value::String(
            "Expected tuple (id, u32)".to_string(),
        )),
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
        Box::new(TcpHandler::new(tcp_config))
    }

    fn name(&self) -> &str {
        "tcp"
    }

    fn imports(&self) -> Option<Vec<String>> {
        Some(vec!["theater:simple/tcp".to_string()])
    }

    fn exports(&self) -> Option<Vec<String>> {
        Some(vec!["theater:simple/tcp-client".to_string()])
    }

    fn start(
        &mut self,
        actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance,
        mut shutdown_receiver: ShutdownReceiver,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        let listen_addr = self.config.listen.clone();
        let state = self.state.clone();

        Box::pin(async move {
            let Some(listen_addr) = listen_addr else {
                // No listener configured — client-only mode, just wait for shutdown.
                shutdown_receiver.wait_for_shutdown().await;
                info!("TCP handler (client-only) received shutdown signal");
                return Ok(());
            };

            let Some(state) = state else {
                error!("TCP handler has no state — setup_host_functions_composite not called?");
                return Ok(());
            };

            // Bind the listener and store it for the actor to use via accept().
            let listener = TcpListener::bind(&listen_addr).await?;
            info!("TCP handler listening on {}", listen_addr);

            loop {
                tokio::select! {
                    _ = &mut shutdown_receiver.receiver => {
                        info!("TCP handler received shutdown signal");
                        break;
                    }
                    result = listener.accept() => {
                        match result {
                            Ok((stream, peer_addr)) => {
                                if let Some(max) = state.max_connections {
                                    let count = state.connections.lock().await.len();
                                    if count >= max as usize {
                                        info!("TCP handler rejecting connection from {} (limit {}/{})", peer_addr, count, max);
                                        drop(stream);
                                        continue;
                                    }
                                }
                                info!("TCP handler accepted connection from {}", peer_addr);
                                let conn_id = state.next_id();
                                let conn_id_str = id_to_string(conn_id);
                                state.connections.lock().await.insert(conn_id, stream);

                                let params = Value::Tuple(vec![Value::String(conn_id_str)]);
                                if let Err(e) = actor_handle
                                    .call_function(
                                        "theater:simple/tcp-client.handle-connection".to_string(),
                                        params,
                                    )
                                    .await
                                {
                                    error!("Error calling handle-connection: {}", e);
                                }
                            }
                            Err(e) => {
                                error!("TCP accept error: {}", e);
                            }
                        }
                    }
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
        info!("Setting up TCP host functions (Pack)");

        if ctx.is_satisfied("theater:simple/tcp") {
            info!("theater:simple/tcp already satisfied, skipping");
            return Ok(());
        }

        let state = TcpState::new(self.config.max_connections);
        self.state = Some(state.clone());

        // Clone state for each closure
        let st_connect = state.clone();
        let st_listen = state.clone();
        let st_accept = state.clone();
        let st_send = state.clone();
        let st_receive = state.clone();
        let st_close = state.clone();
        let st_close_listener = state.clone();

        builder
            .interface("theater:simple/tcp")?
            // connect(address: string) -> result<string, string>
            .func_async_result(
                "connect",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let st = st_connect.clone();
                    async move {
                        let address = parse_string(&input)?;
                        st.check_connection_limit().await?;
                        debug!("tcp connect to {}", address);
                        let stream = TcpStream::connect(&address)
                            .await
                            .map_err(|e| Value::String(e.to_string()))?;
                        let id = st.next_id();
                        st.connections.lock().await.insert(id, stream);
                        debug!("tcp connected to {} as conn={}", address, id);
                        Ok::<Value, Value>(Value::String(id_to_string(id)))
                    }
                },
            )?
            // listen(address: string) -> result<string, string>
            .func_async_result(
                "listen",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let st = st_listen.clone();
                    async move {
                        let address = parse_string(&input)?;
                        debug!("tcp listen on {}", address);
                        let listener = TcpListener::bind(&address)
                            .await
                            .map_err(|e| Value::String(e.to_string()))?;
                        let id = st.next_id();
                        st.listeners.lock().await.insert(id, listener);
                        debug!("tcp listening on {} as listener={}", address, id);
                        Ok::<Value, Value>(Value::String(id_to_string(id)))
                    }
                },
            )?
            // accept(listener-id: string) -> result<string, string>
            .func_async_result(
                "accept",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let st = st_accept.clone();
                    async move {
                        let listener_id_str = parse_string(&input)?;
                        let listener_id = string_to_id(&listener_id_str)?;
                        debug!("tcp accept on listener={}", listener_id);

                        let mut listeners = st.listeners.lock().await;
                        let listener = listeners
                            .get_mut(&listener_id)
                            .ok_or_else(|| Value::String(format!("Listener not found: {}", listener_id_str)))?;

                        let (stream, peer_addr) = listener
                            .accept()
                            .await
                            .map_err(|e| Value::String(e.to_string()))?;

                        let conn_id = st.next_id();
                        // Drop listeners before locking connections to avoid potential deadlock
                        drop(listeners);
                        st.connections.lock().await.insert(conn_id, stream);
                        debug!("tcp accepted conn={} from {}", conn_id, peer_addr);
                        Ok::<Value, Value>(Value::String(id_to_string(conn_id)))
                    }
                },
            )?
            // send(connection-id: string, data: list<u8>) -> result<u64, string>
            .func_async_result(
                "send",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let st = st_send.clone();
                    async move {
                        let (conn_id_str, data) = parse_string_and_bytes(&input)?;
                        let conn_id = string_to_id(&conn_id_str)?;
                        let len = data.len();

                        let mut connections = st.connections.lock().await;
                        let stream = connections
                            .get_mut(&conn_id)
                            .ok_or_else(|| Value::String(format!("Connection not found: {}", conn_id_str)))?;

                        stream
                            .write_all(&data)
                            .await
                            .map_err(|e| Value::String(e.to_string()))?;

                        debug!("tcp send conn={} {} bytes", conn_id, len);
                        Ok::<Value, Value>(Value::U64(len as u64))
                    }
                },
            )?
            // receive(connection-id: string, max-bytes: u32) -> result<list<u8>, string>
            .func_async_result(
                "receive",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let st = st_receive.clone();
                    async move {
                        let (conn_id_str, max_bytes) = parse_string_and_u32(&input)?;
                        let conn_id = string_to_id(&conn_id_str)?;

                        let mut connections = st.connections.lock().await;
                        let stream = connections
                            .get_mut(&conn_id)
                            .ok_or_else(|| Value::String(format!("Connection not found: {}", conn_id_str)))?;

                        let mut buf = vec![0u8; max_bytes as usize];
                        let n = stream
                            .read(&mut buf)
                            .await
                            .map_err(|e| Value::String(e.to_string()))?;

                        debug!("tcp receive conn={} {} bytes (max={})", conn_id, n, max_bytes);
                        buf.truncate(n);
                        Ok::<Value, Value>(Value::List {
                            elem_type: ValueType::U8,
                            items: buf.into_iter().map(Value::U8).collect(),
                        })
                    }
                },
            )?
            // close(connection-id: string) -> result<_, string>
            .func_async_result(
                "close",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let st = st_close.clone();
                    async move {
                        let conn_id_str = parse_string(&input)?;
                        let conn_id = string_to_id(&conn_id_str)?;

                        let mut connections = st.connections.lock().await;
                        let stream = connections
                            .remove(&conn_id)
                            .ok_or_else(|| Value::String(format!("Connection not found: {}", conn_id_str)))?;

                        drop(stream);
                        debug!("tcp close conn={}", conn_id);
                        Ok::<Value, Value>(Value::Tuple(vec![]))
                    }
                },
            )?
            // close-listener(listener-id: string) -> result<_, string>
            .func_async_result(
                "close-listener",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let st = st_close_listener.clone();
                    async move {
                        let listener_id_str = parse_string(&input)?;
                        let listener_id = string_to_id(&listener_id_str)?;

                        let mut listeners = st.listeners.lock().await;
                        let listener = listeners
                            .remove(&listener_id)
                            .ok_or_else(|| Value::String(format!("Listener not found: {}", listener_id_str)))?;

                        drop(listener);
                        debug!("tcp close listener={}", listener_id);
                        Ok::<Value, Value>(Value::Tuple(vec![]))
                    }
                },
            )?;

        ctx.mark_satisfied("theater:simple/tcp");
        info!("TCP host functions (Pack) set up successfully");
        Ok(())
    }

    fn register_exports_composite(&self, instance: &mut PackInstance) -> anyhow::Result<()> {
        instance.register_export("theater:simple/tcp-client", "handle-connection");
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
    fn test_tcp_handler_clone() {
        let config = TcpHandlerConfig {
            listen: Some("0.0.0.0:8080".to_string()),
            max_connections: Some(50),
        };
        let handler = TcpHandler::new(config);
        let cloned = handler.create_instance(None);

        assert_eq!(cloned.name(), "tcp");
    }
}

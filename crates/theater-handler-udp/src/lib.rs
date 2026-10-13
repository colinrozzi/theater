//! Raw UDP datagram networking handler for Theater actors.
//!
//! This implements the COMPLETE UDP socket feature set and nothing on top: no
//! reliability/ARQ, no QUIC, no NAT hole-punching, no application framing.
//! Those are protocols built *on* this handler (in guest actors), never inside
//! it. See `udp.pact` for the interface.
//!
//! Design notes carried from the TCP handler's hard-won lessons:
//! - Active-mode delivery is BOUNDED and lossy-by-design: a per-socket recv
//!   loop reads datagrams into a bounded channel; when the actor can't keep up
//!   the datagram is DROPPED (counter incremented), never buffered unbounded.
//!   UDP is lossy by contract, so dropping is correct — and it structurally
//!   prevents the op-loop flood-saturation that is possible with a sequential
//!   actor.
//! - Recv-loop task handles are stored and cancel+JOINed on shutdown (the #225
//!   pattern) so socket fds release deterministically before the handler acks.

use serde::{Deserialize, Serialize};
use socket2::{Domain, Protocol, SockRef, Socket, Type};
use std::collections::HashMap;
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use theater::actor::handle::ActorHandle;
use theater::config::actor_manifest::HandlerConfig;
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::id::TheaterId;
use theater::pack_bridge::{
    pact_result_host_fn, parse_pact, InterfaceImpl, TypeHash, Value, ValueType,
};
use theater::shutdown::ShutdownReceiver;

/// Max UDP payload (IPv4). recv buffers never exceed this.
const MAX_DATAGRAM: usize = 65_536;
/// Bounded in-flight on-datagram deliveries per active socket. Beyond this the
/// recv loop drops (lossy-correct) rather than buffer unbounded.
const ACTIVE_DELIVERY_CAP: usize = 256;

// ============================================================================
// Config (also carries the permission/limit surface — see the module note in
// the PR: UDP follows the TCP handler's precedent of config-based limits rather
// than wiring into the global HandlerPermission enum).
// ============================================================================

/// Configuration + permission limits for the UDP handler.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct UdpHandlerConfig {
    /// Allowed local bind ports. `None` = any port. Port 0 (ephemeral) is
    /// always allowed.
    #[serde(default)]
    pub allowed_bind_ports: Option<Vec<u16>>,
    /// Allowed peer destinations as "host" or "host:port" prefixes for send-to/
    /// send/connect. `None` = any peer. A datagram to a non-allowed peer errors.
    #[serde(default)]
    pub allowed_peers: Option<Vec<String>>,
    /// Allowed multicast groups for join-multicast. `None` = any group.
    #[serde(default)]
    pub allowed_multicast_groups: Option<Vec<String>>,
    /// Max simultaneously open sockets for this actor. `None` = unlimited.
    #[serde(default)]
    pub max_sockets: Option<u32>,
}

// ============================================================================

/// Drops at scope exit, emitting one `phase=… elapsed_ms=…` debug line per host
/// fn invocation regardless of return path.
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
            "udp phase complete",
        );
    }
}

const UDP_PACT: &str = include_str!("../udp.pact");

/// Declare the `theater:simple/udp` interface from the embedded pact.
fn udp_interface() -> InterfaceImpl {
    let pact = parse_pact(UDP_PACT).expect("embedded udp.pact should be valid");
    InterfaceImpl::from_pact(&pact)
}

// ============================================================================
// Socket state
// ============================================================================

/// Receive mode (mirrors the tcp handler's Erlang-style model).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DataMode {
    Passive,
    Active,
    Once,
}

/// A tracked UDP socket with ownership + mode.
struct SocketEntry {
    socket: Arc<UdpSocket>,
    owner: TheaterId,
    data_mode: DataMode,
    /// Set by `connect`; the OS also filters recv to this peer. `None` =
    /// unconnected.
    connected_peer: Option<String>,
    /// Per-socket passive `recv-from` timeout, ms; `0` = block indefinitely.
    recv_timeout_ms: AtomicU64,
    /// Datagrams dropped by active-mode backpressure (surfaced for tests/ops).
    dropped: Arc<AtomicU64>,
}

impl SocketEntry {
    fn recv_timeout(&self) -> Option<std::time::Duration> {
        match self.recv_timeout_ms.load(Ordering::Relaxed) {
            0 => None,
            ms => Some(std::time::Duration::from_millis(ms)),
        }
    }
}

/// Shared UDP state across all handler instances in a runtime.
struct SharedUdpState {
    sockets: Mutex<HashMap<u64, SocketEntry>>,
    next_id: AtomicU64,
    /// Per active-socket recv loop: `socket_id -> (owner, JoinHandle)`. The
    /// socket fd lives in both the entry's Arc and the recv loop; joining the
    /// loop on shutdown (after cancel) drops its Arc so the fd releases
    /// deterministically before the handler acks (the #225 pattern).
    recv_tasks: Mutex<HashMap<u64, (TheaterId, tokio::task::JoinHandle<()>)>>,
}

impl SharedUdpState {
    fn new() -> Self {
        Self {
            sockets: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            recv_tasks: Mutex::new(HashMap::new()),
        }
    }
    fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }
}

// ============================================================================
// Handler
// ============================================================================

/// Handler providing raw UDP datagram networking to WebAssembly actors.
#[derive(Clone)]
pub struct UdpHandler {
    config: UdpHandlerConfig,
    shared_state: Arc<SharedUdpState>,
    actor_id: Arc<std::sync::Mutex<Option<TheaterId>>>,
    actor_handle: Arc<std::sync::Mutex<Option<ActorHandle>>>,
    cancellation_token: CancellationToken,
}

impl UdpHandler {
    pub fn new(config: UdpHandlerConfig) -> Self {
        Self {
            config,
            shared_state: Arc::new(SharedUdpState::new()),
            actor_id: Arc::new(std::sync::Mutex::new(None)),
            actor_handle: Arc::new(std::sync::Mutex::new(None)),
            cancellation_token: CancellationToken::new(),
        }
    }

    pub fn interfaces(&self) -> Vec<InterfaceImpl> {
        vec![udp_interface()]
    }
}

// ── parsing helpers ─────────────────────────────────────────────────────────

fn parse_string(input: &Value) -> Result<String, Value> {
    match input {
        Value::String(s) => Ok(s.clone()),
        Value::Tuple(f) if f.len() == 1 => match &f[0] {
            Value::String(s) => Ok(s.clone()),
            _ => Err(Value::String("expected string".into())),
        },
        _ => Err(Value::String("expected string".into())),
    }
}

/// Pull the Nth tuple field as a String.
fn str_at(fields: &[Value], i: usize) -> Result<String, Value> {
    match fields.get(i) {
        Some(Value::String(s)) => Ok(s.clone()),
        _ => Err(Value::String(format!("expected string at arg {i}"))),
    }
}
fn u32_at(fields: &[Value], i: usize) -> Result<u32, Value> {
    match fields.get(i) {
        Some(Value::U32(n)) => Ok(*n),
        _ => Err(Value::String(format!("expected u32 at arg {i}"))),
    }
}
fn u64_at(fields: &[Value], i: usize) -> Result<u64, Value> {
    match fields.get(i) {
        Some(Value::U64(n)) => Ok(*n),
        _ => Err(Value::String(format!("expected u64 at arg {i}"))),
    }
}
fn bool_at(fields: &[Value], i: usize) -> Result<bool, Value> {
    match fields.get(i) {
        Some(Value::Bool(b)) => Ok(*b),
        _ => Err(Value::String(format!("expected bool at arg {i}"))),
    }
}
fn bytes_at(fields: &[Value], i: usize) -> Result<Vec<u8>, Value> {
    match fields.get(i) {
        Some(Value::List { items, .. }) => Ok(items
            .iter()
            .filter_map(|v| if let Value::U8(b) = v { Some(*b) } else { None })
            .collect()),
        _ => Err(Value::String(format!("expected list<u8> at arg {i}"))),
    }
}
fn tuple(input: &Value, n: usize) -> Result<&[Value], Value> {
    match input {
        Value::Tuple(f) if f.len() == n => Ok(f),
        _ => Err(Value::String(format!("expected tuple of {n} args"))),
    }
}

fn id_to_string(id: u64) -> String {
    id.to_string()
}
fn string_to_id(s: &str) -> Result<u64, Value> {
    s.parse::<u64>()
        .map_err(|_| Value::String(format!("invalid socket-id: {s}")))
}
fn parse_addr(s: &str) -> Result<SocketAddr, Value> {
    s.parse::<SocketAddr>()
        .map_err(|_| Value::String(format!("invalid address: {s}")))
}

/// Value ctors for the success side (pact_result_host_fn wraps as result::ok).
fn ok_unit() -> Value {
    Value::Tuple(vec![])
}
fn ok_bytes_and_peer(data: Vec<u8>, peer: String) -> Value {
    Value::Tuple(vec![
        Value::List {
            elem_type: ValueType::U8,
            items: data.into_iter().map(Value::U8).collect(),
        },
        Value::String(peer),
    ])
}

// ── bind + socket options (socket2 for what tokio doesn't expose) ───────────

/// Bind a UDP socket with SO_REUSEADDR on (clean rebind after restart) and
/// dual-stack for a v6 wildcard. Non-blocking + a tokio socket.
fn bind_udp(addr: SocketAddr) -> std::io::Result<UdpSocket> {
    let domain = if addr.is_ipv4() {
        Domain::IPV4
    } else {
        Domain::IPV6
    };
    let sock = Socket::new(domain, Type::DGRAM, Some(Protocol::UDP))?;
    sock.set_reuse_address(true)?;
    if addr.is_ipv6() {
        // Best-effort dual-stack; harmless if the OS rejects it for this addr.
        let _ = sock.set_only_v6(false);
    }
    sock.set_nonblocking(true)?;
    sock.bind(&addr.into())?;
    UdpSocket::from_std(sock.into())
}

// ============================================================================
// Active-mode recv loop (bounded, drop-oldest/lossy)
// ============================================================================

/// Per active socket: read datagrams and deliver `on-datagram` to the actor,
/// BOUNDED. Reads into a bounded channel drained by a delivery task; when the
/// actor can't keep up, datagrams are DROPPED (counter incremented), never
/// buffered unbounded. `is_once` delivers exactly one then returns (the caller
/// reverts the socket to passive).
async fn udp_recv_loop(
    socket_id: u64,
    socket: Arc<UdpSocket>,
    actor_handle: ActorHandle,
    dropped: Arc<AtomicU64>,
    is_once: bool,
    cancel: CancellationToken,
) {
    let sid = id_to_string(socket_id);
    info!(
        "udp recv loop started for socket={} (once={})",
        sid, is_once
    );

    // Bounded delivery: recv never blocks on the actor; a full channel => drop.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<(String, Vec<u8>)>(ACTIVE_DELIVERY_CAP);
    let deliver_sid = sid.clone();
    let deliver = tokio::spawn(async move {
        while let Some((peer, data)) = rx.recv().await {
            let params = Value::Tuple(vec![
                Value::String(deliver_sid.clone()),
                Value::String(peer),
                Value::List {
                    elem_type: ValueType::U8,
                    items: data.into_iter().map(Value::U8).collect(),
                },
            ]);
            if let Err(e) = actor_handle
                .call_function("theater:simple/udp-client.on-datagram".to_string(), params)
                .await
            {
                warn!(
                    "udp socket={} on-datagram delivery failed: {}",
                    deliver_sid, e
                );
            }
        }
    });

    let mut buf = vec![0u8; MAX_DATAGRAM];
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!("udp recv loop cancelled for socket={}", sid);
                break;
            }
            r = socket.recv_from(&mut buf) => match r {
                Ok((n, src)) => {
                    let data = buf[..n].to_vec();
                    match tx.try_send((src.to_string(), data)) {
                        Ok(()) => {}
                        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                            let d = dropped.fetch_add(1, Ordering::Relaxed) + 1;
                            if d.is_power_of_two() {
                                debug!("udp socket={} dropped {} datagram(s) (actor backpressure)", sid, d);
                            }
                        }
                        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => break,
                    }
                    if is_once {
                        break;
                    }
                }
                Err(e) => {
                    // A recv error on a connected socket is typically an ICMP
                    // port-unreachable; surface it but keep the socket alive.
                    warn!("udp socket={} recv error: {}", sid, e);
                    break;
                }
            }
        }
    }

    drop(tx); // close the channel so the delivery task finishes draining + exits
    let _ = deliver.await;
    debug!("udp recv loop exited for socket={}", sid);
}

/// One passive `recv-from`: receive a complete datagram, interruptible on
/// shutdown and optionally bounded by `timeout`. A datagram larger than
/// `max_bytes` is truncated (BSD recvfrom semantics), capped at the UDP max.
/// Returns `(bytes, sender-addr)`, or `Err("timeout: …")` / `Err("socket
/// closed: …")`.
async fn recv_from_bounded(
    socket: &UdpSocket,
    max_bytes: usize,
    timeout: Option<std::time::Duration>,
    cancel: &CancellationToken,
) -> Result<(Vec<u8>, String), Value> {
    let mut buf = vec![0u8; max_bytes.min(MAX_DATAGRAM)];
    let timeout_fut = async {
        match timeout {
            Some(d) => tokio::time::sleep(d).await,
            None => std::future::pending::<()>().await,
        }
    };
    tokio::pin!(timeout_fut);
    let (n, src) = tokio::select! {
        r = socket.recv_from(&mut buf) => r.map_err(|e| Value::String(e.to_string()))?,
        _ = cancel.cancelled() =>
            return Err(Value::String("socket closed: actor shutting down".into())),
        _ = &mut timeout_fut => {
            let ms = timeout.map(|d| d.as_millis()).unwrap_or(0);
            return Err(Value::String(format!("timeout: no datagram within {ms}ms")));
        }
    };
    buf.truncate(n);
    Ok((buf, src.to_string()))
}

// ============================================================================
// Permission/limit checks (config-based, enforced here)
// ============================================================================

impl UdpHandlerConfig {
    fn check_bind_port(&self, addr: &SocketAddr) -> Result<(), Value> {
        if addr.port() == 0 {
            return Ok(()); // ephemeral always allowed
        }
        if let Some(ref allowed) = self.allowed_bind_ports {
            if !allowed.contains(&addr.port()) {
                return Err(Value::String(format!(
                    "permission denied: bind port {} not allowed",
                    addr.port()
                )));
            }
        }
        Ok(())
    }
    fn check_peer(&self, peer: &str) -> Result<(), Value> {
        if let Some(ref allowed) = self.allowed_peers {
            if !allowed.iter().any(|a| peer.starts_with(a.as_str())) {
                return Err(Value::String(format!(
                    "permission denied: peer {peer} not allowed"
                )));
            }
        }
        Ok(())
    }
    fn check_group(&self, group: &str) -> Result<(), Value> {
        if let Some(ref allowed) = self.allowed_multicast_groups {
            if !allowed.iter().any(|g| g == group) {
                return Err(Value::String(format!(
                    "permission denied: multicast group {group} not allowed"
                )));
            }
        }
        Ok(())
    }
}

// ============================================================================
// Handler impl
// ============================================================================

impl Handler for UdpHandler {
    fn create_instance(&self, config: Option<&HandlerConfig>) -> Box<dyn Handler> {
        let cfg = match config {
            Some(c) => c.parse::<UdpHandlerConfig>().unwrap_or_else(|e| {
                warn!("invalid udp handler config: {}; using default", e);
                self.config.clone()
            }),
            None => self.config.clone(),
        };
        Box::new(UdpHandler {
            config: cfg,
            shared_state: self.shared_state.clone(), // shared Arc across instances
            actor_id: Arc::new(std::sync::Mutex::new(None)),
            actor_handle: Arc::new(std::sync::Mutex::new(None)),
            cancellation_token: CancellationToken::new(),
        })
    }

    fn name(&self) -> &str {
        "udp"
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
        Some(vec!["theater:simple/udp-client".to_string()])
    }
    fn interface_hashes(&self) -> Vec<(String, TypeHash)> {
        self.interfaces()
            .iter()
            .map(|i| (i.name().to_string(), i.hash()))
            .collect()
    }
    fn interfaces(&self) -> Vec<InterfaceImpl> {
        vec![udp_interface()]
    }

    fn setup(
        &mut self,
        actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance,
        shutdown_receiver: ShutdownReceiver,
        _event_rx: theater::handler::HandlerEventReceiver,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        info!("UDP handler setup");
        {
            let mut g = self.actor_handle.lock().unwrap();
            *g = Some(actor_handle);
        }
        let cancel_token = self.cancellation_token.clone();
        let shared_state = self.shared_state.clone();
        let actor_id_for_cleanup = self.actor_id.clone();

        Box::pin(async move {
            // BIND (do not drop) the shutdown signal: the ack fires on drop, and
            // the runtime awaits it. Hold it past the recv-loop join below so the
            // runtime waits for socket fds to release before treating the handler
            // as down (the #225 fd-release-before-ack discipline).
            let _shutdown_signal = shutdown_receiver.wait_for_shutdown().await;
            info!("UDP handler received shutdown, cleaning up");
            cancel_token.cancel();

            let actor_id_val = *actor_id_for_cleanup.lock().unwrap();

            // Cancel+JOIN this actor's recv loops so their socket Arcs drop
            // (fds released) before we ack via `_shutdown_signal`'s drop.
            let handles: Vec<tokio::task::JoinHandle<()>> = {
                let mut tasks = shared_state.recv_tasks.lock().await;
                let owned: Vec<u64> = tasks
                    .iter()
                    .filter(|(_, (owner, _))| Some(*owner) == actor_id_val)
                    .map(|(id, _)| *id)
                    .collect();
                owned
                    .into_iter()
                    .filter_map(|id| tasks.remove(&id).map(|(_, h)| h))
                    .collect()
            };
            let joined = handles.len();
            for h in handles {
                let _ = h.await;
            }

            // Drop this actor's sockets (releases any passive-mode fds).
            let removed = {
                let mut sockets = shared_state.sockets.lock().await;
                let before = sockets.len();
                sockets.retain(|_, e| Some(e.owner) != actor_id_val);
                before - sockets.len()
            };
            if joined > 0 || removed > 0 {
                info!(
                    "UDP handler cleaned up {} recv loop(s) + {} socket(s) for actor {:?}",
                    joined, removed, actor_id_val
                );
            }
            Ok(())
        })
    }

    fn register_host_functions(
        &mut self,
        imports: &mut theater::pack_bridge::HostImports,
        ctx: &mut HandlerContext,
    ) -> anyhow::Result<()> {
        info!("Setting up UDP host functions");
        if ctx.is_satisfied("theater:simple/udp") {
            return Ok(());
        }
        let actor_id = ctx
            .actor_id
            .ok_or_else(|| anyhow::anyhow!("actor_id not set in HandlerContext"))?;
        {
            let mut g = self.actor_id.lock().unwrap();
            *g = Some(actor_id);
        }

        let st = self.shared_state.clone();
        let cfg = self.config.clone();
        let actor_handle_arc = self.actor_handle.clone();
        let cancel = self.cancellation_token.clone();

        // ---- bind ----
        {
            let st = st.clone();
            let cfg = cfg.clone();
            imports.define(
                "theater:simple/udp",
                "bind",
                pact_result_host_fn(move |input: Value| {
                    let st = st.clone();
                    let cfg = cfg.clone();
                    async move {
                        let _ph = PhaseLog::new("udp.bind");
                        let addr_s = parse_string(&input)?;
                        let addr = parse_addr(&addr_s)?;
                        cfg.check_bind_port(&addr)?;
                        if let Some(max) = cfg.max_sockets {
                            let n = st
                                .sockets
                                .lock()
                                .await
                                .values()
                                .filter(|e| e.owner == actor_id)
                                .count();
                            if n >= max as usize {
                                return Err(Value::String(format!(
                                    "permission denied: max sockets ({max}) reached"
                                )));
                            }
                        }
                        let socket = bind_udp(addr)
                            .map_err(|e| Value::String(format!("bind {addr_s} failed: {e}")))?;
                        let id = st.next_id();
                        st.sockets.lock().await.insert(
                            id,
                            SocketEntry {
                                socket: Arc::new(socket),
                                owner: actor_id,
                                data_mode: DataMode::Passive,
                                connected_peer: None,
                                recv_timeout_ms: AtomicU64::new(0),
                                dropped: Arc::new(AtomicU64::new(0)),
                            },
                        );
                        info!("udp bound {} as socket={}", addr_s, id);
                        Ok::<Value, Value>(Value::String(id_to_string(id)))
                    }
                }),
            );
        }

        // Helper: clone the socket Arc for an owned socket id (validates owner).
        // Used by the simple per-socket verbs below.
        macro_rules! owned_socket {
            ($st:expr, $sid_str:expr) => {{
                let sid = string_to_id(&$sid_str)?;
                let sockets = $st.sockets.lock().await;
                let entry = sockets
                    .get(&sid)
                    .ok_or_else(|| Value::String(format!("socket not found: {}", $sid_str)))?;
                if entry.owner != actor_id {
                    return Err(Value::String(format!(
                        "socket {} not owned by this actor",
                        $sid_str
                    )));
                }
                (sid, entry.socket.clone())
            }};
        }

        // ---- close ----
        {
            let st = st.clone();
            imports.define(
                "theater:simple/udp",
                "close",
                pact_result_host_fn(move |input: Value| {
                    let st = st.clone();
                    async move {
                        let _ph = PhaseLog::new("udp.close");
                        let sid_str = parse_string(&input)?;
                        let sid = string_to_id(&sid_str)?;
                        let removed = {
                            let mut sockets = st.sockets.lock().await;
                            match sockets.get(&sid) {
                                Some(e) if e.owner == actor_id => sockets.remove(&sid).is_some(),
                                Some(_) => {
                                    return Err(Value::String(format!(
                                        "socket {sid_str} not owned by this actor"
                                    )))
                                }
                                None => false,
                            }
                        };
                        // Cancel+drop any recv loop for this socket.
                        if let Some((_, h)) = st.recv_tasks.lock().await.remove(&sid) {
                            h.abort();
                        }
                        if !removed {
                            return Err(Value::String(format!("socket not found: {sid_str}")));
                        }
                        Ok::<Value, Value>(ok_unit())
                    }
                }),
            );
        }

        // ---- connect / disconnect ----
        {
            let st = st.clone();
            let cfg = cfg.clone();
            imports.define(
                "theater:simple/udp",
                "connect",
                pact_result_host_fn(move |input: Value| {
                    let st = st.clone();
                    let cfg = cfg.clone();
                    async move {
                        let _ph = PhaseLog::new("udp.connect");
                        let f = tuple(&input, 2)?;
                        let sid_str = str_at(f, 0)?;
                        let peer = str_at(f, 1)?;
                        cfg.check_peer(&peer)?;
                        let paddr = parse_addr(&peer)?;
                        let (sid, socket) = owned_socket!(st, sid_str);
                        socket
                            .connect(paddr)
                            .await
                            .map_err(|e| Value::String(format!("connect {peer} failed: {e}")))?;
                        if let Some(e) = st.sockets.lock().await.get_mut(&sid) {
                            e.connected_peer = Some(peer.clone());
                        }
                        Ok::<Value, Value>(ok_unit())
                    }
                }),
            );
        }
        {
            let st = st.clone();
            imports.define(
                "theater:simple/udp",
                "disconnect",
                pact_result_host_fn(move |input: Value| {
                    let st = st.clone();
                    async move {
                        let _ph = PhaseLog::new("udp.disconnect");
                        let sid_str = parse_string(&input)?;
                        let (sid, socket) = owned_socket!(st, sid_str);
                        // Connecting to an unspecified addr dissolves the association.
                        let unspec: SocketAddr = (Ipv4Addr::UNSPECIFIED, 0).into();
                        let _ = socket.connect(unspec).await;
                        if let Some(e) = st.sockets.lock().await.get_mut(&sid) {
                            e.connected_peer = None;
                        }
                        Ok::<Value, Value>(ok_unit())
                    }
                }),
            );
        }

        // ---- local-addr / connected-peer ----
        {
            let st = st.clone();
            imports.define(
                "theater:simple/udp",
                "local-addr",
                pact_result_host_fn(move |input: Value| {
                    let st = st.clone();
                    async move {
                        let _ph = PhaseLog::new("udp.local-addr");
                        let sid_str = parse_string(&input)?;
                        let (_sid, socket) = owned_socket!(st, sid_str);
                        let addr = socket
                            .local_addr()
                            .map_err(|e| Value::String(format!("local-addr failed: {e}")))?;
                        Ok::<Value, Value>(Value::String(addr.to_string()))
                    }
                }),
            );
        }
        {
            let st = st.clone();
            imports.define(
                "theater:simple/udp",
                "connected-peer",
                pact_result_host_fn(move |input: Value| {
                    let st = st.clone();
                    async move {
                        let _ph = PhaseLog::new("udp.connected-peer");
                        let sid_str = parse_string(&input)?;
                        let sid = string_to_id(&sid_str)?;
                        let sockets = st.sockets.lock().await;
                        let entry = sockets
                            .get(&sid)
                            .ok_or_else(|| Value::String(format!("socket not found: {sid_str}")))?;
                        if entry.owner != actor_id {
                            return Err(Value::String(format!(
                                "socket {sid_str} not owned by this actor"
                            )));
                        }
                        let peer = entry.connected_peer.clone();
                        Ok::<Value, Value>(Value::Option {
                            inner_type: ValueType::String,
                            value: peer.map(|p| Box::new(Value::String(p))),
                        })
                    }
                }),
            );
        }

        // ---- send-to / send ----
        {
            let st = st.clone();
            let cfg = cfg.clone();
            imports.define(
                "theater:simple/udp",
                "send-to",
                pact_result_host_fn(move |input: Value| {
                    let st = st.clone();
                    let cfg = cfg.clone();
                    async move {
                        let _ph = PhaseLog::new("udp.send-to");
                        let f = tuple(&input, 3)?;
                        let sid_str = str_at(f, 0)?;
                        let peer = str_at(f, 1)?;
                        let data = bytes_at(f, 2)?;
                        cfg.check_peer(&peer)?;
                        let paddr = parse_addr(&peer)?;
                        let (_sid, socket) = owned_socket!(st, sid_str);
                        let n = socket
                            .send_to(&data, paddr)
                            .await
                            .map_err(|e| Value::String(format!("send-to {peer} failed: {e}")))?;
                        Ok::<Value, Value>(Value::U64(n as u64))
                    }
                }),
            );
        }
        {
            let st = st.clone();
            imports.define(
                "theater:simple/udp",
                "send",
                pact_result_host_fn(move |input: Value| {
                    let st = st.clone();
                    async move {
                        let _ph = PhaseLog::new("udp.send");
                        let f = tuple(&input, 2)?;
                        let sid_str = str_at(f, 0)?;
                        let data = bytes_at(f, 1)?;
                        let (_sid, socket) = owned_socket!(st, sid_str);
                        let n = socket.send(&data).await.map_err(|e| {
                            Value::String(format!("send failed (socket connected?): {e}"))
                        })?;
                        Ok::<Value, Value>(Value::U64(n as u64))
                    }
                }),
            );
        }

        // ---- recv-from (passive, bounded by per-socket recv-timeout) ----
        {
            let st = st.clone();
            let cancel = cancel.clone();
            imports.define(
                "theater:simple/udp",
                "recv-from",
                pact_result_host_fn(move |input: Value| {
                    let st = st.clone();
                    let cancel = cancel.clone();
                    async move {
                        let _ph = PhaseLog::new("udp.recv-from");
                        let f = tuple(&input, 2)?;
                        let sid_str = str_at(f, 0)?;
                        let max_bytes = u32_at(f, 1)? as usize;
                        let sid = string_to_id(&sid_str)?;
                        let (socket, timeout) = {
                            let sockets = st.sockets.lock().await;
                            let entry = sockets.get(&sid).ok_or_else(|| {
                                Value::String(format!("socket not found: {sid_str}"))
                            })?;
                            if entry.owner != actor_id {
                                return Err(Value::String(format!(
                                    "socket {sid_str} not owned by this actor"
                                )));
                            }
                            if entry.data_mode != DataMode::Passive {
                                return Err(Value::String(format!(
                                    "socket {sid_str} is in active/once mode - datagrams are pushed to on-datagram"
                                )));
                            }
                            (entry.socket.clone(), entry.recv_timeout())
                        };
                        let (data, peer) =
                            recv_from_bounded(&socket, max_bytes, timeout, &cancel).await?;
                        Ok::<Value, Value>(ok_bytes_and_peer(data, peer))
                    }
                }),
            );
        }

        // ---- set-active ----
        {
            let st = st.clone();
            let actor_handle_arc = actor_handle_arc.clone();
            let cancel = cancel.clone();
            imports.define(
                "theater:simple/udp",
                "set-active",
                pact_result_host_fn(move |input: Value| {
                    let st = st.clone();
                    let actor_handle_arc = actor_handle_arc.clone();
                    let cancel = cancel.clone();
                    async move {
                        let _ph = PhaseLog::new("udp.set-active");
                        let f = tuple(&input, 2)?;
                        let sid_str = str_at(f, 0)?;
                        let mode_s = str_at(f, 1)?;
                        let new_mode = match mode_s.as_str() {
                            "passive" => DataMode::Passive,
                            "active" => DataMode::Active,
                            "once" => DataMode::Once,
                            _ => {
                                return Err(Value::String(format!(
                                    "invalid mode {mode_s:?} (want passive|active|once)"
                                )))
                            }
                        };
                        let sid = string_to_id(&sid_str)?;
                        let (socket, dropped) = {
                            let mut sockets = st.sockets.lock().await;
                            let entry = sockets.get_mut(&sid).ok_or_else(|| {
                                Value::String(format!("socket not found: {sid_str}"))
                            })?;
                            if entry.owner != actor_id {
                                return Err(Value::String(format!(
                                    "socket {sid_str} not owned by this actor"
                                )));
                            }
                            let prev = entry.data_mode;
                            entry.data_mode = new_mode;
                            if prev == new_mode {
                                return Ok::<Value, Value>(ok_unit());
                            }
                            (entry.socket.clone(), entry.dropped.clone())
                        };

                        // Going passive: cancel/stop any running recv loop.
                        if new_mode == DataMode::Passive {
                            if let Some((_, h)) = st.recv_tasks.lock().await.remove(&sid) {
                                h.abort();
                            }
                            return Ok::<Value, Value>(ok_unit());
                        }

                        // active/once: (re)spawn the recv loop.
                        let actor_handle = {
                            let g = actor_handle_arc.lock().unwrap();
                            g.clone()
                        };
                        let Some(actor_handle) = actor_handle else {
                            return Err(Value::String("actor handle not available".into()));
                        };
                        let is_once = new_mode == DataMode::Once;
                        let st_for_task = st.clone();
                        let cancel_for_task = cancel.clone();
                        let handle = tokio::spawn(async move {
                            udp_recv_loop(
                                sid,
                                socket,
                                actor_handle,
                                dropped,
                                is_once,
                                cancel_for_task,
                            )
                            .await;
                            // once-mode self-reverts to passive when the loop ends.
                            if is_once {
                                if let Some(e) = st_for_task.sockets.lock().await.get_mut(&sid) {
                                    if e.data_mode == DataMode::Once {
                                        e.data_mode = DataMode::Passive;
                                    }
                                }
                                st_for_task.recv_tasks.lock().await.remove(&sid);
                            }
                        });
                        if let Some((_, old)) =
                            st.recv_tasks.lock().await.insert(sid, (actor_id, handle))
                        {
                            old.abort(); // replace any prior loop
                        }
                        info!("udp socket={} set to {} mode", sid_str, mode_s);
                        Ok::<Value, Value>(ok_unit())
                    }
                }),
            );
        }

        // ---- set-recv-timeout ----
        {
            let st = st.clone();
            imports.define(
                "theater:simple/udp",
                "set-recv-timeout",
                pact_result_host_fn(move |input: Value| {
                    let st = st.clone();
                    async move {
                        let _ph = PhaseLog::new("udp.set-recv-timeout");
                        let f = tuple(&input, 2)?;
                        let sid_str = str_at(f, 0)?;
                        let ms = u64_at(f, 1)?;
                        let sid = string_to_id(&sid_str)?;
                        let sockets = st.sockets.lock().await;
                        let entry = sockets
                            .get(&sid)
                            .ok_or_else(|| Value::String(format!("socket not found: {sid_str}")))?;
                        if entry.owner != actor_id {
                            return Err(Value::String(format!(
                                "socket {sid_str} not owned by this actor"
                            )));
                        }
                        entry.recv_timeout_ms.store(ms, Ordering::Relaxed);
                        Ok::<Value, Value>(ok_unit())
                    }
                }),
            );
        }

        // ---- socket options: ttl / broadcast / tos / bufsizes ----
        macro_rules! opt_verb {
            ($name:literal, $phase:literal, $body:expr) => {{
                let st = st.clone();
                imports.define(
                    "theater:simple/udp",
                    $name,
                    pact_result_host_fn(move |input: Value| {
                        let st = st.clone();
                        async move {
                            let _ph = PhaseLog::new($phase);
                            let f = tuple(&input, 2)?;
                            let sid_str = str_at(f, 0)?;
                            let (_sid, socket) = owned_socket!(st, sid_str);
                            let apply: &dyn Fn(&UdpSocket, &[Value]) -> Result<(), Value> = &$body;
                            apply(&socket, f)?;
                            Ok::<Value, Value>(ok_unit())
                        }
                    }),
                );
            }};
        }

        opt_verb!("set-ttl", "udp.set-ttl", |sock: &UdpSocket, f: &[Value]| {
            let ttl = u32_at(f, 1)?;
            if sock.local_addr().map(|a| a.is_ipv4()).unwrap_or(true) {
                sock.set_ttl(ttl).map_err(|e| Value::String(e.to_string()))
            } else {
                SockRef::from(sock)
                    .set_unicast_hops_v6(ttl)
                    .map_err(|e| Value::String(e.to_string()))
            }
        });
        opt_verb!(
            "set-broadcast",
            "udp.set-broadcast",
            |sock: &UdpSocket, f: &[Value]| {
                let on = bool_at(f, 1)?;
                sock.set_broadcast(on)
                    .map_err(|e| Value::String(e.to_string()))
            }
        );
        opt_verb!("set-tos", "udp.set-tos", |sock: &UdpSocket, f: &[Value]| {
            let tos = u32_at(f, 1)?;
            SockRef::from(sock)
                .set_tos(tos)
                .map_err(|e| Value::String(e.to_string()))
        });
        opt_verb!(
            "set-send-buffer-size",
            "udp.set-send-buffer-size",
            |sock: &UdpSocket, f: &[Value]| {
                let n = u32_at(f, 1)? as usize;
                SockRef::from(sock)
                    .set_send_buffer_size(n)
                    .map_err(|e| Value::String(e.to_string()))
            }
        );
        opt_verb!(
            "set-recv-buffer-size",
            "udp.set-recv-buffer-size",
            |sock: &UdpSocket, f: &[Value]| {
                let n = u32_at(f, 1)? as usize;
                SockRef::from(sock)
                    .set_recv_buffer_size(n)
                    .map_err(|e| Value::String(e.to_string()))
            }
        );
        opt_verb!(
            "set-multicast-loop",
            "udp.set-multicast-loop",
            |sock: &UdpSocket, f: &[Value]| {
                let on = bool_at(f, 1)?;
                if sock.local_addr().map(|a| a.is_ipv4()).unwrap_or(true) {
                    sock.set_multicast_loop_v4(on)
                        .map_err(|e| Value::String(e.to_string()))
                } else {
                    sock.set_multicast_loop_v6(on)
                        .map_err(|e| Value::String(e.to_string()))
                }
            }
        );
        opt_verb!(
            "set-multicast-ttl",
            "udp.set-multicast-ttl",
            |sock: &UdpSocket, f: &[Value]| {
                let ttl = u32_at(f, 1)?;
                if sock.local_addr().map(|a| a.is_ipv4()).unwrap_or(true) {
                    sock.set_multicast_ttl_v4(ttl)
                        .map_err(|e| Value::String(e.to_string()))
                } else {
                    SockRef::from(sock)
                        .set_multicast_hops_v6(ttl)
                        .map_err(|e| Value::String(e.to_string()))
                }
            }
        );

        // ---- multicast join / leave ----
        {
            let st = st.clone();
            let cfg = cfg.clone();
            imports.define(
                "theater:simple/udp",
                "join-multicast",
                pact_result_host_fn(move |input: Value| {
                    let st = st.clone();
                    let cfg = cfg.clone();
                    async move {
                        let _ph = PhaseLog::new("udp.join-multicast");
                        let f = tuple(&input, 3)?;
                        let sid_str = str_at(f, 0)?;
                        let group = str_at(f, 1)?;
                        let iface = str_at(f, 2)?;
                        cfg.check_group(&group)?;
                        let (_sid, socket) = owned_socket!(st, sid_str);
                        join_or_leave(&socket, &group, &iface, true)?;
                        Ok::<Value, Value>(ok_unit())
                    }
                }),
            );
        }
        {
            let st = st.clone();
            imports.define(
                "theater:simple/udp",
                "leave-multicast",
                pact_result_host_fn(move |input: Value| {
                    let st = st.clone();
                    async move {
                        let _ph = PhaseLog::new("udp.leave-multicast");
                        let f = tuple(&input, 3)?;
                        let sid_str = str_at(f, 0)?;
                        let group = str_at(f, 1)?;
                        let iface = str_at(f, 2)?;
                        let (_sid, socket) = owned_socket!(st, sid_str);
                        join_or_leave(&socket, &group, &iface, false)?;
                        Ok::<Value, Value>(ok_unit())
                    }
                }),
            );
        }

        Ok(())
    }

    fn supports_composite(&self) -> bool {
        true
    }
}

/// Join or leave a multicast group, family-aware. For v4 `interface` is a local
/// IPv4 address ("0.0.0.0" = default); for v6 it is a numeric scope-id ("0" =
/// default).
fn join_or_leave(
    socket: &UdpSocket,
    group: &str,
    interface: &str,
    join: bool,
) -> Result<(), Value> {
    let gaddr: IpAddr = group
        .parse()
        .map_err(|_| Value::String(format!("invalid multicast group: {group}")))?;
    match gaddr {
        IpAddr::V4(g) => {
            let iface: Ipv4Addr = if interface.is_empty() {
                Ipv4Addr::UNSPECIFIED
            } else {
                interface
                    .parse()
                    .map_err(|_| Value::String(format!("invalid v4 interface: {interface}")))?
            };
            let r = if join {
                socket.join_multicast_v4(g, iface)
            } else {
                socket.leave_multicast_v4(g, iface)
            };
            r.map_err(|e| Value::String(e.to_string()))
        }
        IpAddr::V6(g) => {
            let scope: u32 = if interface.is_empty() {
                0
            } else {
                interface.parse().map_err(|_| {
                    Value::String(format!("invalid v6 interface scope: {interface}"))
                })?
            };
            let r = if join {
                socket.join_multicast_v6(&g, scope)
            } else {
                socket.leave_multicast_v6(&g, scope)
            };
            r.map_err(|e| Value::String(e.to_string()))
        }
    }
}

#[cfg(test)]
mod tests;

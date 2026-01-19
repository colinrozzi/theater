//! Backing types for WASI Sockets resources
//!
//! These types represent the actual socket resources that are managed by Theater.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use tokio::net::{TcpListener, TcpSocket as TokioTcpSocket, TcpStream, UdpSocket as TokioUdpSocket};

/// Network resource - represents access to the network
///
/// In Theater, we provide a single default network that allows
/// all networking operations (subject to actor permissions).
#[derive(Debug, Clone)]
pub struct Network {
    /// Unique identifier for this network instance
    pub id: u32,
}

impl Network {
    /// Create the default network instance
    pub fn default_network() -> Self {
        Self { id: 0 }
    }
}

/// TCP socket state machine
///
/// Note: Manual Debug impl because TokioTcpSocket doesn't implement Debug
pub enum TcpSocketState {
    /// Socket created but not bound
    Unbound,
    /// Bind operation in progress
    BindInProgress {
        local_address: SocketAddr,
    },
    /// Socket bound to local address (holds the actual OS socket)
    Bound {
        socket: TokioTcpSocket,
        local_address: SocketAddr,
    },
    /// Listen operation in progress (holds the socket to convert to listener)
    ListenInProgress {
        socket: TokioTcpSocket,
        local_address: SocketAddr,
    },
    /// Socket listening for connections
    Listening {
        listener: TcpListener,
        local_address: SocketAddr,
    },
    /// Connect operation in progress
    ConnectInProgress {
        local_address: Option<SocketAddr>,
        remote_address: SocketAddr,
    },
    /// Socket connected to peer
    Connected {
        stream: TcpStream,
        local_address: SocketAddr,
        remote_address: SocketAddr,
    },
    /// Socket closed
    Closed,
}

impl std::fmt::Debug for TcpSocketState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TcpSocketState::Unbound => write!(f, "Unbound"),
            TcpSocketState::BindInProgress { local_address } => {
                f.debug_struct("BindInProgress")
                    .field("local_address", local_address)
                    .finish()
            }
            TcpSocketState::Bound { local_address, .. } => {
                f.debug_struct("Bound")
                    .field("local_address", local_address)
                    .field("socket", &"<TokioTcpSocket>")
                    .finish()
            }
            TcpSocketState::ListenInProgress { local_address, .. } => {
                f.debug_struct("ListenInProgress")
                    .field("local_address", local_address)
                    .field("socket", &"<TokioTcpSocket>")
                    .finish()
            }
            TcpSocketState::Listening { local_address, .. } => {
                f.debug_struct("Listening")
                    .field("local_address", local_address)
                    .field("listener", &"<TcpListener>")
                    .finish()
            }
            TcpSocketState::ConnectInProgress { local_address, remote_address } => {
                f.debug_struct("ConnectInProgress")
                    .field("local_address", local_address)
                    .field("remote_address", remote_address)
                    .finish()
            }
            TcpSocketState::Connected { local_address, remote_address, .. } => {
                f.debug_struct("Connected")
                    .field("local_address", local_address)
                    .field("remote_address", remote_address)
                    .field("stream", &"<TcpStream>")
                    .finish()
            }
            TcpSocketState::Closed => write!(f, "Closed"),
        }
    }
}

/// TCP socket resource
#[derive(Debug)]
pub struct TcpSocket {
    /// Address family (IPv4 or IPv6)
    pub family: IpAddressFamily,
    /// Current socket state
    pub state: TcpSocketState,
    /// Socket options
    pub options: TcpSocketOptions,
}

/// TCP socket options
#[derive(Debug, Clone)]
pub struct TcpSocketOptions {
    pub keep_alive_enabled: bool,
    pub keep_alive_idle_time: u64, // nanoseconds
    pub keep_alive_interval: u64,  // nanoseconds
    pub keep_alive_count: u32,
    pub hop_limit: u8,
    pub receive_buffer_size: u64,
    pub send_buffer_size: u64,
    pub listen_backlog_size: u64,
}

impl Default for TcpSocketOptions {
    fn default() -> Self {
        Self {
            keep_alive_enabled: false,
            keep_alive_idle_time: 7200_000_000_000, // 2 hours in nanoseconds
            keep_alive_interval: 75_000_000_000,    // 75 seconds
            keep_alive_count: 9,
            hop_limit: 64,
            receive_buffer_size: 65536,
            send_buffer_size: 65536,
            listen_backlog_size: 128,
        }
    }
}

impl TcpSocket {
    /// Create a new unbound TCP socket
    pub fn new(family: IpAddressFamily) -> Self {
        Self {
            family,
            state: TcpSocketState::Unbound,
            options: TcpSocketOptions::default(),
        }
    }

    /// Check if the socket is in the listening state
    pub fn is_listening(&self) -> bool {
        matches!(self.state, TcpSocketState::Listening { .. })
    }

    /// Get the local address if bound
    pub fn local_address(&self) -> Option<SocketAddr> {
        match &self.state {
            TcpSocketState::Bound { local_address, .. } => Some(*local_address),
            TcpSocketState::ListenInProgress { local_address, .. } => Some(*local_address),
            TcpSocketState::Listening { local_address, .. } => Some(*local_address),
            TcpSocketState::ConnectInProgress { local_address, .. } => *local_address,
            TcpSocketState::Connected { local_address, .. } => Some(*local_address),
            _ => None,
        }
    }

    /// Get the remote address if connected
    pub fn remote_address(&self) -> Option<SocketAddr> {
        match &self.state {
            TcpSocketState::Connected { remote_address, .. } => Some(*remote_address),
            _ => None,
        }
    }
}

/// UDP socket state
#[derive(Debug)]
pub enum UdpSocketState {
    /// Socket created but not bound
    Unbound,
    /// Bind operation in progress
    BindInProgress {
        local_address: SocketAddr,
    },
    /// Socket bound and ready
    Bound {
        socket: Arc<TokioUdpSocket>,
        local_address: SocketAddr,
        remote_address: Option<SocketAddr>,
    },
    /// Socket closed
    Closed,
}

/// UDP socket resource
#[derive(Debug)]
pub struct UdpSocket {
    /// Address family (IPv4 or IPv6)
    pub family: IpAddressFamily,
    /// Current socket state
    pub state: UdpSocketState,
    /// Socket options
    pub options: UdpSocketOptions,
}

/// UDP socket options
#[derive(Debug, Clone)]
pub struct UdpSocketOptions {
    pub unicast_hop_limit: u8,
    pub receive_buffer_size: u64,
    pub send_buffer_size: u64,
}

impl Default for UdpSocketOptions {
    fn default() -> Self {
        Self {
            unicast_hop_limit: 64,
            receive_buffer_size: 65536,
            send_buffer_size: 65536,
        }
    }
}

impl UdpSocket {
    /// Create a new unbound UDP socket
    pub fn new(family: IpAddressFamily) -> Self {
        Self {
            family,
            state: UdpSocketState::Unbound,
            options: UdpSocketOptions::default(),
        }
    }

    /// Get the local address if bound
    pub fn local_address(&self) -> Option<SocketAddr> {
        match &self.state {
            UdpSocketState::Bound { local_address, .. } => Some(*local_address),
            _ => None,
        }
    }

    /// Get the remote address if connected
    pub fn remote_address(&self) -> Option<SocketAddr> {
        match &self.state {
            UdpSocketState::Bound { remote_address, .. } => *remote_address,
            _ => None,
        }
    }
}

/// Incoming datagram stream for UDP receive operations
#[derive(Debug)]
pub struct IncomingDatagramStream {
    /// Reference to the UDP socket
    pub socket: Arc<TokioUdpSocket>,
    /// Remote address filter (if connected mode)
    pub remote_address: Option<SocketAddr>,
}

/// Outgoing datagram stream for UDP send operations
#[derive(Debug)]
pub struct OutgoingDatagramStream {
    /// Reference to the UDP socket
    pub socket: Arc<TokioUdpSocket>,
    /// Remote address (if connected mode)
    pub remote_address: Option<SocketAddr>,
    /// Number of datagrams permitted to send
    pub send_permit: u64,
}

/// DNS resolution stream
#[derive(Debug)]
pub struct ResolveAddressStream {
    /// The addresses resolved (populated asynchronously)
    pub addresses: Vec<IpAddr>,
    /// Current position in the address list
    pub position: usize,
    /// Whether resolution is complete
    pub complete: bool,
    /// Error if resolution failed
    pub error: Option<String>,
}

impl ResolveAddressStream {
    /// Create a new resolve stream
    pub fn new() -> Self {
        Self {
            addresses: Vec::new(),
            position: 0,
            complete: false,
            error: None,
        }
    }

    /// Get the next address
    pub fn next_address(&mut self) -> Option<IpAddr> {
        if self.position < self.addresses.len() {
            let addr = self.addresses[self.position];
            self.position += 1;
            Some(addr)
        } else {
            None
        }
    }
}

/// IP address family
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpAddressFamily {
    Ipv4,
    Ipv6,
}

impl IpAddressFamily {
    /// Get the unspecified address for this family
    pub fn unspecified_address(&self) -> IpAddr {
        match self {
            IpAddressFamily::Ipv4 => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            IpAddressFamily::Ipv6 => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
        }
    }
}

/// Helper to convert from WIT IP address to std::net
pub fn wit_ip_to_std(_family: IpAddressFamily, addr: &crate::bindings::wasi::sockets::network::IpAddress) -> IpAddr {
    match addr {
        crate::bindings::wasi::sockets::network::IpAddress::Ipv4((a, b, c, d)) => {
            IpAddr::V4(Ipv4Addr::new(*a, *b, *c, *d))
        }
        crate::bindings::wasi::sockets::network::IpAddress::Ipv6((a, b, c, d, e, f, g, h)) => {
            IpAddr::V6(Ipv6Addr::new(*a, *b, *c, *d, *e, *f, *g, *h))
        }
    }
}

/// Helper to convert from std::net IP address to WIT
pub fn std_ip_to_wit(addr: IpAddr) -> crate::bindings::wasi::sockets::network::IpAddress {
    match addr {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            crate::bindings::wasi::sockets::network::IpAddress::Ipv4((
                octets[0], octets[1], octets[2], octets[3],
            ))
        }
        IpAddr::V6(v6) => {
            let segments = v6.segments();
            crate::bindings::wasi::sockets::network::IpAddress::Ipv6((
                segments[0], segments[1], segments[2], segments[3],
                segments[4], segments[5], segments[6], segments[7],
            ))
        }
    }
}

/// Helper to convert from WIT socket address to std::net
pub fn wit_socket_addr_to_std(addr: &crate::bindings::wasi::sockets::network::IpSocketAddress) -> SocketAddr {
    match addr {
        crate::bindings::wasi::sockets::network::IpSocketAddress::Ipv4(v4) => {
            let ip = Ipv4Addr::new(v4.address.0, v4.address.1, v4.address.2, v4.address.3);
            SocketAddr::new(IpAddr::V4(ip), v4.port)
        }
        crate::bindings::wasi::sockets::network::IpSocketAddress::Ipv6(v6) => {
            let ip = Ipv6Addr::new(
                v6.address.0, v6.address.1, v6.address.2, v6.address.3,
                v6.address.4, v6.address.5, v6.address.6, v6.address.7,
            );
            SocketAddr::new(IpAddr::V6(ip), v6.port)
        }
    }
}

/// Helper to convert from std::net socket address to WIT
pub fn std_socket_addr_to_wit(addr: SocketAddr) -> crate::bindings::wasi::sockets::network::IpSocketAddress {
    match addr {
        SocketAddr::V4(v4) => {
            let octets = v4.ip().octets();
            crate::bindings::wasi::sockets::network::IpSocketAddress::Ipv4(
                crate::bindings::wasi::sockets::network::Ipv4SocketAddress {
                    port: v4.port(),
                    address: (octets[0], octets[1], octets[2], octets[3]),
                }
            )
        }
        SocketAddr::V6(v6) => {
            let segments = v6.ip().segments();
            crate::bindings::wasi::sockets::network::IpSocketAddress::Ipv6(
                crate::bindings::wasi::sockets::network::Ipv6SocketAddress {
                    port: v6.port(),
                    flow_info: v6.flowinfo(),
                    address: (
                        segments[0], segments[1], segments[2], segments[3],
                        segments[4], segments[5], segments[6], segments[7],
                    ),
                    scope_id: v6.scope_id(),
                }
            )
        }
    }
}

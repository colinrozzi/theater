//! Event types for WASI Sockets operations
//!
//! These events are logged to Theater's event chain to track all socket operations
//! for replay and verification purposes.

use serde::{Deserialize, Serialize};

/// Event data for WASI Sockets operations
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SocketsEventData {
    // === Network Events ===
    /// instance-network called
    InstanceNetworkCall,
    /// instance-network result
    InstanceNetworkResult { network_id: u32 },

    // === TCP Socket Creation ===
    /// create-tcp-socket called
    CreateTcpSocketCall { family: String },
    /// create-tcp-socket result
    CreateTcpSocketResult { socket_id: u32, success: bool },

    // === TCP Bind ===
    /// tcp-socket.start-bind called
    TcpStartBindCall { address: String },
    /// tcp-socket.start-bind result
    TcpStartBindResult { success: bool },
    /// tcp-socket.finish-bind called
    TcpFinishBindCall,
    /// tcp-socket.finish-bind result
    TcpFinishBindResult { success: bool },

    // === TCP Connect ===
    /// tcp-socket.start-connect called
    TcpStartConnectCall { address: String },
    /// tcp-socket.start-connect result
    TcpStartConnectResult { success: bool },
    /// tcp-socket.finish-connect called
    TcpFinishConnectCall,
    /// tcp-socket.finish-connect result
    TcpFinishConnectResult { success: bool },

    // === TCP Listen ===
    /// tcp-socket.start-listen called
    TcpStartListenCall,
    /// tcp-socket.start-listen result
    TcpStartListenResult { success: bool },
    /// tcp-socket.finish-listen called
    TcpFinishListenCall,
    /// tcp-socket.finish-listen result
    TcpFinishListenResult { success: bool },

    // === TCP Accept ===
    /// tcp-socket.accept called
    TcpAcceptCall,
    /// tcp-socket.accept result
    TcpAcceptResult { client_socket_id: u32, success: bool },

    // === TCP Shutdown ===
    /// tcp-socket.shutdown called
    TcpShutdownCall { shutdown_type: String },
    /// tcp-socket.shutdown result
    TcpShutdownResult { success: bool },

    // === TCP Address Queries ===
    /// tcp-socket.local-address called
    TcpLocalAddressCall,
    /// tcp-socket.local-address result
    TcpLocalAddressResult { address: Option<String> },
    /// tcp-socket.remote-address called
    TcpRemoteAddressCall,
    /// tcp-socket.remote-address result
    TcpRemoteAddressResult { address: Option<String> },

    // === UDP Socket Creation ===
    /// create-udp-socket called
    CreateUdpSocketCall { family: String },
    /// create-udp-socket result
    CreateUdpSocketResult { socket_id: u32, success: bool },

    // === UDP Bind ===
    /// udp-socket.start-bind called
    UdpStartBindCall { address: String },
    /// udp-socket.start-bind result
    UdpStartBindResult { success: bool },
    /// udp-socket.finish-bind called
    UdpFinishBindCall,
    /// udp-socket.finish-bind result
    UdpFinishBindResult { success: bool },

    // === UDP Stream ===
    /// udp-socket.stream called
    UdpStreamCall { remote_address: Option<String> },
    /// udp-socket.stream result
    UdpStreamResult { success: bool },

    // === UDP Send/Receive ===
    /// incoming-datagram-stream.receive called
    UdpReceiveCall { max_results: u64 },
    /// incoming-datagram-stream.receive result
    UdpReceiveResult { datagrams_received: usize },
    /// outgoing-datagram-stream.check-send called
    UdpCheckSendCall,
    /// outgoing-datagram-stream.check-send result
    UdpCheckSendResult { permit: u64 },
    /// outgoing-datagram-stream.send called
    UdpSendCall { num_datagrams: usize },
    /// outgoing-datagram-stream.send result
    UdpSendResult { datagrams_sent: u64 },

    // === DNS Resolution ===
    /// resolve-addresses called
    ResolveAddressesCall { name: String },
    /// resolve-addresses result
    ResolveAddressesResult { success: bool },
    /// resolve-address-stream.resolve-next-address called
    ResolveNextAddressCall,
    /// resolve-address-stream.resolve-next-address result
    ResolveNextAddressResult { address: Option<String> },

    // === Socket Options ===
    /// Socket option get/set
    SocketOptionGet { option: String, value: String },
    SocketOptionSet { option: String, value: String, success: bool },
}

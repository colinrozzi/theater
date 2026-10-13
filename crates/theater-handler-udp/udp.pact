// Theater UDP Interface
//
// Raw UDP datagram networking for actors. This is the COMPLETE UDP socket
// feature set and DELIBERATELY NOTHING MORE: no reliability/ARQ, no QUIC, no
// NAT hole-punching, no application framing. Those are protocols built ON TOP
// of this handler (in guest actors or a separate handler), never inside it.
//
// Socket lifecycle:
//   1. bind(address) -> socket-id   (an unconnected datagram socket)
//   2. send-to()/recv-from() to arbitrary peers, OR connect() to filter+address
//      a single peer and then send()/recv-from().
//   3. close(socket-id).
//
// Receive modes (mirrors the tcp handler's model):
//   - "passive" (default): pull datagrams with recv-from().
//   - "active": each datagram pushed to the on-datagram callback.
//   - "once": one on-datagram callback, then back to passive.
// set-active() changes the mode. Active delivery is BOUNDED and drop-oldest:
// UDP is lossy by contract, so a peer flooding faster than the sequential actor
// drains is dropped (with a surfaced counter), never buffered unbounded.
//
// Callback interface the ACTOR implements (export side, theater:simple/udp-client):
//   on-datagram: func(socket-id: string, peer: string, data: list<u8>) -> result<_, string>
//   on-recv-error: func(socket-id: string, reason: string) -> result<_, string>
//     (e.g. an ICMP port-unreachable on a connected socket; informational)
//
// Permissions (manifest [[handler]] type = "udp"): allowed local bind ports,
// allowed peer address/port ranges, allowed multicast groups, max open sockets,
// and an optional send rate-limit — UDP's spoofing/amplification surface makes
// these load-bearing, same spirit as the tcp handler's permission model.

interface udp {
    @package: string = "theater:simple"

    exports {
        // ---- socket lifecycle ----

        // Bind a UDP socket to a local address ("host:port"; port 0 = ephemeral,
        // "[::]:p"/"0.0.0.0:p" = wildcard; v6 address => dual-stack unless v6-only
        // is set). SO_REUSEADDR is on by default so a rebind after restart does
        // not hit TIME_WAIT. Returns the socket-id.
        bind: func(address: string) -> result<string, string>

        // Close + drop a socket (releases the fd).
        close: func(socket-id: string) -> result<_, string>

        // ---- addressing ----

        // "Connect" the socket to a single peer: subsequent recv-from only
        // delivers datagrams from that peer, and send() (no addr) is enabled.
        // Also lets the OS surface ICMP errors as on-recv-error. Re-connecting
        // changes the peer; disconnect() clears it.
        connect: func(socket-id: string, peer: string) -> result<_, string>
        disconnect: func(socket-id: string) -> result<_, string>

        // The socket's bound local address (resolves an ephemeral port 0).
        local-addr: func(socket-id: string) -> result<string, string>
        // The connected peer, if any.
        connected-peer: func(socket-id: string) -> result<option<string>, string>

        // ---- send ----

        // Send one datagram to an explicit peer. Returns bytes sent.
        send-to: func(socket-id: string, peer: string, data: list<u8>) -> result<u64, string>
        // Send one datagram to the connected peer (socket must be connect()ed).
        send: func(socket-id: string, data: list<u8>) -> result<u64, string>

        // ---- receive ----

        // Passive: receive ONE complete datagram, returning (bytes, sender-addr).
        // A datagram larger than max-bytes is truncated (BSD recvfrom semantics);
        // size max-bytes to your protocol's max (UDP payload max is 65507 v4).
        // Fails if the socket is in active/once mode.
        recv-from: func(socket-id: string, max-bytes: u32) -> result<tuple<list<u8>, string>, string>

        // Switch receive mode: "passive" | "active" | "once" (see header).
        set-active: func(socket-id: string, mode: string) -> result<_, string>

        // Bound how long a passive recv-from() blocks waiting for a datagram.
        // 0 = disabled (block indefinitely, the default). Non-zero => recv-from
        // returns Err("timeout: ...") if no datagram arrives in time, so a
        // sequential actor cannot be wedged by a silent socket. Per-socket.
        set-recv-timeout: func(socket-id: string, timeout-ms: u64) -> result<_, string>

        // ---- socket options ----

        // Unicast TTL / hop-limit for outgoing datagrams.
        set-ttl: func(socket-id: string, ttl: u32) -> result<_, string>
        // Allow sending to the broadcast address (SO_BROADCAST).
        set-broadcast: func(socket-id: string, enabled: bool) -> result<_, string>
        // IP TOS / DSCP byte for QoS marking.
        set-tos: func(socket-id: string, tos: u32) -> result<_, string>
        // OS send/receive buffer sizes (SO_SNDBUF / SO_RCVBUF), in bytes.
        set-send-buffer-size: func(socket-id: string, bytes: u32) -> result<_, string>
        set-recv-buffer-size: func(socket-id: string, bytes: u32) -> result<_, string>

        // ---- multicast ----

        // Join / leave a multicast group on the given local interface
        // ("0.0.0.0" / "::" = default interface). After joining, datagrams sent
        // to the group arrive via recv-from / on-datagram.
        join-multicast: func(socket-id: string, group: string, interface: string) -> result<_, string>
        leave-multicast: func(socket-id: string, group: string, interface: string) -> result<_, string>
        // Loopback of this socket's own multicast sends back to it.
        set-multicast-loop: func(socket-id: string, enabled: bool) -> result<_, string>
        // TTL / hop-limit for outgoing multicast datagrams.
        set-multicast-ttl: func(socket-id: string, ttl: u32) -> result<_, string>
    }
}

// Theater TCP Interface
//
// Raw TCP networking for actors.
// Provides connect, listen, accept, send, receive, and close operations.
//
// Connection lifecycle:
// 1. accept() returns a connection in PENDING state (no data flows yet)
// 2. Call activate() to start using the connection, OR
// 3. Call transfer() to hand the connection to another actor
//
// Data modes (Erlang-style):
// - "passive": Data only received via explicit receive() calls (default after activate)
// - "active": Data pushed to on-data callback continuously
// - "once": Receive one chunk via on-data, then switch to passive
//
// Use set-active() to change the data mode after activation.

interface tcp {
    @package: string = "theater:simple"

    exports {
        // Connect to a remote TCP address, returns connection ID
        // Connection is immediately active in passive data mode
        connect: func(address: string) -> result<string, string>

        // Start listening on address, returns listener ID
        listen: func(address: string) -> result<string, string>

        // Accept connection from listener, blocks until connection arrives
        // Returns connection in PENDING state - must call activate() or transfer()
        accept: func(listener-id: string) -> result<string, string>

        // Activate a pending connection so this actor can send/receive on it
        // Connection starts in passive data mode (use set-active to change)
        activate: func(connection-id: string) -> result<_, string>

        // Set data mode for a connection: "active", "once", or "passive"
        // - "passive": use receive() to get data (default)
        // - "active": data pushed to on-data callback continuously
        // - "once": one on-data callback, then switches to passive
        set-active: func(connection-id: string, mode: string) -> result<_, string>

        // Transfer a connection to another actor
        // Connection is activated in passive data mode for the target actor
        transfer: func(connection-id: string, target-actor: string) -> result<_, string>

        // Non-blocking transfer: flips ownership to the target and activates the
        // connection, then dispatches the target's handle-connection-transfer in
        // a detached task and returns immediately (without awaiting the target's
        // handler lifecycle). Use when one acceptor hands off many connections and
        // must not serialize on each target completing. If the target's handler
        // errors or traps, the connection is closed and dropped.
        transfer-async: func(connection-id: string, target-actor: string) -> result<_, string>

        // Get the peer address of a connection (works in pending or active state)
        peer-address: func(connection-id: string) -> result<string, string>

        // Send data on connection, returns bytes written
        // Fails if connection is pending or not owned by this actor
        send: func(connection-id: string, data: list<u8>) -> result<u64, string>

        // Receive up to max-bytes, empty list means EOF
        // Fails if connection is pending, not owned, or in active/once mode
        receive: func(connection-id: string, max-bytes: u32) -> result<list<u8>, string>

        // Close a connection
        close: func(connection-id: string) -> result<_, string>

        // Close a listener
        close-listener: func(listener-id: string) -> result<_, string>

        // Upgrade an existing plain TCP connection to TLS as the server side.
        // Uses the server_tls cert/key configured on this handler. The actor
        // is expected to have already exchanged whatever protocol greeting
        // negotiates the upgrade (e.g. SMTP STARTTLS, IMAP STARTTLS).
        // After this returns Ok, send/receive on this connection-id flow
        // over TLS — same connection-id, encrypted bytes.
        upgrade-to-tls-server: func(connection-id: string) -> result<_, string>

        // Upgrade an existing plain TCP connection to TLS as the client side.
        // server-name is the hostname used for SNI and cert verification.
        // Uses the client_tls config on this handler.
        upgrade-to-tls-client: func(connection-id: string, server-name: string) -> result<_, string>
    }
}

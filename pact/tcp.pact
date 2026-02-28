// Theater TCP Interface
//
// Raw TCP networking for actors.
// Provides connect, listen, accept, send, receive, and close operations.
//
// Connection lifecycle:
// 1. accept() returns a connection in PENDING state (no data flows yet)
// 2. Call activate() to receive data yourself, OR
// 3. Call transfer() to hand the connection to another actor
//
// This allows safe connection handoff without data races.

interface tcp {
    @package: string = "theater:simple"

    exports {
        // Connect to a remote TCP address, returns connection ID
        // Connection is immediately active (no pending state for outbound)
        connect: func(address: string) -> result<string, string>

        // Start listening on address, returns listener ID
        listen: func(address: string) -> result<string, string>

        // Accept connection from listener, blocks until connection arrives
        // Returns connection in PENDING state - must call activate() or transfer()
        accept: func(listener-id: string) -> result<string, string>

        // Activate a pending connection so this actor can send/receive on it
        activate: func(connection-id: string) -> result<_, string>

        // Transfer a connection to another actor
        // Connection is automatically activated for the target actor
        transfer: func(connection-id: string, target-actor: string) -> result<_, string>

        // Get the peer address of a connection (works in pending or active state)
        peer-address: func(connection-id: string) -> result<string, string>

        // Send data on connection, returns bytes written
        // Fails if connection is pending or not owned by this actor
        send: func(connection-id: string, data: list<u8>) -> result<u64, string>

        // Receive up to max-bytes, empty list means EOF
        // Fails if connection is pending or not owned by this actor
        receive: func(connection-id: string, max-bytes: u32) -> result<list<u8>, string>

        // Close a connection
        close: func(connection-id: string) -> result<_, string>

        // Close a listener
        close-listener: func(listener-id: string) -> result<_, string>
    }
}

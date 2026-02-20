// Theater TCP Interface
//
// Raw TCP networking for actors.
// Provides connect, listen, accept, send, receive, and close operations.

interface tcp {
    @package: string = "theater:simple"

    exports {
        // Connect to a remote TCP address, returns connection ID
        connect: func(address: string) -> result<string, string>

        // Start listening on address, returns listener ID
        listen: func(address: string) -> result<string, string>

        // Accept connection from listener, blocks until connection arrives
        accept: func(listener-id: string) -> result<string, string>

        // Send data on connection, returns bytes written
        send: func(connection-id: string, data: list<u8>) -> result<u64, string>

        // Receive up to max-bytes, empty list means EOF
        receive: func(connection-id: string, max-bytes: u32) -> result<list<u8>, string>

        // Close a connection
        close: func(connection-id: string) -> result<_, string>

        // Close a listener
        close-listener: func(listener-id: string) -> result<_, string>
    }
}

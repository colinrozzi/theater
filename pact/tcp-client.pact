// Theater TCP Client Interface
//
// Actor exports for handling TCP events.

interface tcp-client {
    @package: string = "theater:simple"

    exports {
        // Called when new connection accepted on configured listener
        // params: (connection-id)
        handle-connection: func(state: option<list<u8>>, params: tuple<string>) -> result<tuple<option<list<u8>>>, string>

        // Called when data arrives on a connection in active/once mode
        // params: (connection-id, data)
        // Return empty tuple on success, error string on failure
        on-data: func(connection-id: string, data: list<u8>) -> result<_, string>

        // Called when a connection is closed (EOF or error)
        // params: (connection-id, reason) where reason is "eof" or error message
        on-close: func(connection-id: string, reason: string) -> result<_, string>
    }
}

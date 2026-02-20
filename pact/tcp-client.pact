// Theater TCP Client Interface
//
// Actor export for handling incoming TCP connections.

interface tcp-client {
    @package: string = "theater:simple"

    exports {
        // Called when new connection accepted on configured listener
        handle-connection: func(state: option<list<u8>>, params: tuple<string>) -> result<tuple<option<list<u8>>>, string>
    }
}

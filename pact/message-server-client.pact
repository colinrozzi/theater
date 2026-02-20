// Theater Message Server Client Interface
//
// Handlers actors must implement to receive messages.

interface message-server-client {
    @package: string = "theater:simple"

    use types.{channel-accept}

    exports {
        // Handle one-way message
        handle-send: func(state: option<list<u8>>, params: tuple<list<u8>>) -> result<tuple<option<list<u8>>>, string>

        // Handle request-response
        handle-request: func(state: option<list<u8>>, params: tuple<string, list<u8>>) -> result<tuple<option<list<u8>>, tuple<option<list<u8>>>>, string>

        // Handle channel open request
        handle-channel-open: func(state: option<list<u8>>, params: tuple<string, list<u8>>) -> result<tuple<option<list<u8>>, tuple<channel-accept>>, string>

        // Handle message on channel
        handle-channel-message: func(state: option<list<u8>>, params: tuple<string, list<u8>>) -> result<tuple<option<list<u8>>>, string>

        // Handle channel close
        handle-channel-close: func(state: option<list<u8>>, params: tuple<string>) -> result<tuple<option<list<u8>>>, string>
    }
}

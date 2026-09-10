// Theater Message Server Client Interface
//
// Handlers actors must implement to receive messages. In-module state: the
// runtime no longer threads actor state through these callbacks — each takes
// only its own arguments and returns only its own result (state lives in the
// actor's module; see docs/in-module-state.md). Where a callback previously
// returned only new state, the ok branch collapses to unit; where it returned
// state + a real payload, only the payload remains.

interface message-server-client {
    @package: string = "theater:simple"

    use types.{channel-accept}

    exports {
        // Handle a one-way message.
        handle-send: func(message: list<u8>) -> result<_, string>

        // Handle a request; ok = the optional response bytes.
        handle-request: func(request-id: string, message: list<u8>) -> result<option<list<u8>>, string>

        // Handle a channel-open request; ok = the accept/reject decision.
        handle-channel-open: func(channel-id: string, message: list<u8>) -> result<channel-accept, string>

        // Handle a message on an open channel.
        handle-channel-message: func(channel-id: string, message: list<u8>) -> result<_, string>

        // Handle a channel close.
        handle-channel-close: func(channel-id: string) -> result<_, string>
    }
}

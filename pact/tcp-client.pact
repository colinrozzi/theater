// Theater TCP Client Interface
//
// Actor exports for handling TCP events. In-module state: the runtime no longer
// threads actor state through these callbacks — each takes only its own
// arguments and returns only its own result (state lives in the actor's module;
// see docs/in-module-state.md).

interface tcp-client {
    @package: string = "theater:simple"

    exports {
        // Called when a new connection is accepted on the configured listener.
        handle-connection: func(connection-id: string) -> result<_, string>

        // Called on the TARGET actor after a connection is handed to it via
        // tcp.transfer / transfer-async. Declared here (not just invoked by
        // name) so the interface hash covers it — the runtime invokes it on the
        // handler, so it is part of the tcp-client contract.
        handle-connection-transfer: func(connection-id: string) -> result<_, string>

        // Called when data arrives on a connection in active/once mode.
        on-data: func(connection-id: string, data: list<u8>) -> result<_, string>

        // Called when a connection is closed (EOF or error). `reason` is "eof"
        // or an error message.
        on-close: func(connection-id: string, reason: string) -> result<_, string>
    }
}

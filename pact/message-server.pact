// Theater Message Server Host Interface
//
// Functions actors can call for inter-actor messaging.

interface message-server-host {
    @package: string = "theater:simple"

    exports {
        // Send one-way message to another actor
        send: func(actor-id: string, msg: list<u8>) -> result<_, string>

        // Send request and await response
        request: func(actor-id: string, msg: list<u8>) -> result<list<u8>, string>

        // Open bidirectional channel with another actor
        open-channel: func(actor-id: string, initial-msg: list<u8>) -> result<string, string>

        // Send message on established channel
        send-on-channel: func(channel-id: string, msg: list<u8>) -> result<_, string>

        // Close a channel
        close-channel: func(channel-id: string) -> result<_, string>

        // List pending request IDs
        list-outstanding-requests: func() -> list<string>

        // Respond to a specific request
        respond-to-request: func(request-id: string, response: list<u8>) -> result<_, string>

        // Cancel a pending request
        cancel-request: func(request-id: string) -> result<_, string>
    }
}

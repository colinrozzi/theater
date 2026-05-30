// Supervisor Handler Callbacks
//
// Exports that actors should implement to receive notifications about child lifecycle events.
// These are called by the supervisor handler when children report events.
interface supervisor-handlers {
    @package: string = "theater:simple"

    // Simplified error type for WIT compatibility
    enum wit-error-type {
        operation-timeout,
        channel-closed,
        shutting-down,
        function-not-found,
        type-mismatch,
        internal,
        serialization-error,
        update-component-error,
        paused
    }

    record wit-actor-error {
        error-type: wit-error-type,
        data: option<list<u8>>
    }

    exports {
        // Called when a child actor encounters an error
        handle-child-error: func(child-id: string, error: wit-actor-error) -> result<_, string>

        // Called when a child actor exits successfully
        handle-child-exit: func(child-id: string, result: option<list<u8>>) -> result<_, string>

        // Called when a child actor is stopped externally (e.g., by stop-child or system shutdown)
        handle-child-external-stop: func(child-id: string) -> result<_, string>

        // Called for every event a child records (optional - may not be implemented)
        // child-id: the id of the child that recorded the event
        // event-type: the type string of the event
        // event-data: the serialized event data
        //
        // child-id lets supervisors of N children attribute events to the
        // child they came from. The other handle-child-* callbacks already
        // carry child-id; this one was historically untagged because the
        // supervisor's event-subscription channel was multiplexed at the
        // handler boundary. The handler now tags events per-child before
        // dispatching, so every callback in this interface is keyed by
        // child-id.
        handle-child-event: func(child-id: string, event-type: string, event-data: list<u8>) -> result<_, string>
    }
}

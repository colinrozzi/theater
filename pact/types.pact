// Theater Common Types
//
// Shared types used across Theater interfaces.

interface types {
    @package: string = "theater:simple"

    // Actor identifier
    type actor-id = string

    // Channel identifier
    type channel-id = string

    // Response to channel connection request
    record channel-accept {
        accepted: bool,
        message: option<list<u8>>,
    }

    // Complete event chain for an actor
    record chain {
        events: list<meta-event>,
    }

    // Event with metadata (hash)
    record meta-event {
        hash: u64,
        event: event,
    }

    // Core event structure
    record event {
        event-type: string,
        parent: option<u64>,
        data: list<u8>,
    }

    // Event in chain with full metadata
    record chain-event {
        hash: list<u8>,
        parent-hash: option<list<u8>>,
        event-type: string,
        data: list<u8>,
        timestamp: u64,
        description: option<string>,
    }

    // Actor error info
    record actor-error {
        error-type: error-type,
        data: option<list<u8>>,
    }

    // Error types
    enum error-type {
        operation-timeout,
        channel-closed,
        shutting-down,
        function-not-found,
        type-mismatch,
        internal,
        serialization-error,
        update-component-error,
        paused,
    }
}

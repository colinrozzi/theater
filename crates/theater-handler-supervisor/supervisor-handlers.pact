// Theater Supervisor Callback Interface
//
// The EXPORT side of supervision: functions the ACTOR implements and the host
// (supervisor handler) calls to deliver events about actors in the actor's
// view. Formalizing this here — it used to be string-only and unhashed — makes
// host and actor agree on one hashed contract, so drift becomes a hash mismatch
// instead of a silent runtime miss.

interface supervisor-handlers {
    @package: string = "theater:simple"

    // A structured failure reported for a WATCHED actor (the actor that died —
    // distinct from a supervisor op failure). Replaces the old
    // enum-plus-data-bytes wit-actor-error workaround with a real variant.
    variant actor-error {
        operation-timeout(string),
        channel-closed,
        shutting-down,
        function-not-found(string),
        type-mismatch(string),
        serialization-error,
        paused,
        internal(string),
    }

    exports {
        // A chain event recorded by a watched actor (via subscribe-to-actor).
        handle-actor-event: func(id: string, event-type: string, data: list<u8>) -> result<_, string>

        // A watched actor errored (terminal); `error` carries the structured cause.
        handle-actor-error: func(id: string, error: actor-error) -> result<_, string>

        // A watched actor exited cleanly (terminal); `result` = final state, if any.
        handle-actor-exit: func(id: string, result: option<list<u8>>) -> result<_, string>

        // A watched actor was stopped by something other than this supervisor.
        handle-actor-external-stop: func(id: string) -> result<_, string>
    }
}

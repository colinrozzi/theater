// Theater Lifecycle-Relationship Interface
//
// Actor-facing surface over the runtime's subscription substrate. An actor
// attaches a directed relationship to another actor (the "subject") — always
// as itself (self-service; the runtime sets subscriber = caller):
//
//   - link:    fate-sharing. When the subject terminates, the runtime stops
//              the caller (a StopSelf subscription). No wasm callback.
//   - monitor: watching. Matching subject events are delivered to the caller's
//              `handle-lifecycle-event` export (a DeliverToWasm subscription).
//
// Default filters are fixed: link keys on any termination, monitor on any
// lifecycle event. `monitor-filtered` lets the caller supply its own structural
// (packr_abi::Pattern) filter to narrow the delivered stream.

interface lifecycle {
    @package: string = "theater:simple"

    exports {
        // Fate-link the caller to `subject` (an actor id). When `subject`
        // terminates, the caller is stopped by the runtime.
        link: func(subject: string) -> result<_, string>

        // Remove the caller's fate-link to `subject`.
        unlink: func(subject: string) -> result<_, string>

        // Monitor `subject`: its lifecycle events are delivered to the caller's
        // `handle-lifecycle-event` export.
        monitor: func(subject: string) -> result<_, string>

        // Monitor `subject` with a caller-supplied filter, so a supervisor can
        // narrow the stream (e.g. terminations-only, or Failed-only) instead of
        // receiving every lifecycle event — cutting monitor amplification under
        // high spawn/death churn. `filter` is a serialized `packr_abi::Pattern`
        // (its `From`/`TryFrom<Value>`) matched host-side against the subject's
        // decoded ChainEventPayload. Delivery is otherwise identical to
        // `monitor`: Target::DeliverToWasm, the same `handle-lifecycle-event`
        // callback, still within the lifecycle-event scope (the handler's
        // LIFECYCLE_EVENT_TYPES pre-filter is unchanged).
        monitor-filtered: func(subject: string, filter: value) -> result<_, string>

        // Remove the caller's monitor of `subject`.
        unmonitor: func(subject: string) -> result<_, string>

        // Subscribe to an actor's lifecycle events, delivered to the caller's
        // `handle-lifecycle-event` export. A monitor by another name (moved here
        // from the former supervisor interface). Opt-in and idempotent.
        subscribe-to-actor: func(id: string) -> result<_, string>

        // Stop receiving events from an actor. Idempotent; subscriptions are
        // also auto-released when the actor exits.
        unsubscribe-from-actor: func(id: string) -> result<_, string>
    }
}

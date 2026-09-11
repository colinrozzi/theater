// Theater Lifecycle-Relationship Interface
//
// Actor-facing surface over the runtime's subscription substrate. An actor
// attaches a directed relationship to another actor (the "subject") — always
// as itself (self-service; the runtime sets subscriber = caller):
//
//   - link:    fate-sharing. When the subject terminates, the runtime stops
//              the caller (a StopSelf subscription). No wasm callback.
//   - monitor: watching. The subject's chain events are delivered to the
//              caller's `handle-actor-event` export (a DeliverToWasm
//              subscription).
//
// `link` keys on any termination; `monitor` watches the WHOLE chain (every
// chain event of the subject, not just lifecycle events). `monitor-filtered`
// lets the caller supply its own structural (packr_abi::Pattern) filter to
// narrow the delivered stream to just the events it cares about.

interface lifecycle {
    @package: string = "theater:simple"

    exports {
        // Fate-link the caller to `subject` (an actor id). When `subject`
        // terminates, the caller is stopped by the runtime.
        link: func(subject: string) -> result<_, string>

        // Remove the caller's fate-link to `subject`.
        unlink: func(subject: string) -> result<_, string>

        // Monitor `subject`'s WHOLE chain: every chain event of the subject is
        // delivered to the caller's `handle-actor-event` export. The chain is
        // the actor's source of truth, so a bare monitor watches all of it;
        // use `monitor-filtered` to narrow the stream.
        monitor: func(subject: string) -> result<_, string>

        // Monitor `subject` with a caller-supplied filter, so a watcher can
        // narrow the stream (e.g. terminations-only, or lifecycle-only) instead
        // of receiving every chain event — cutting monitor amplification under
        // high event churn. `filter` is a serialized `packr_abi::Pattern` (its
        // `From`/`TryFrom<Value>`) matched host-side against the subject's
        // decoded ChainEventPayload. Delivery is otherwise identical to
        // `monitor`: Target::DeliverToWasm, the same `handle-actor-event`
        // callback.
        monitor-filtered: func(subject: string, filter: value) -> result<_, string>

        // Remove the caller's monitor of `subject`.
        unmonitor: func(subject: string) -> result<_, string>
    }
}

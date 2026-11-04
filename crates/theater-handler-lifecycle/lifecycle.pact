// Theater Lifecycle-Relationship Interface
//
// Actor-facing surface over the runtime's subscription substrate. An actor
// attaches a directed relationship to another actor (the "subject") — always
// as itself (self-service; the runtime sets subscriber = caller):
//
//   - link:    fate-sharing. When the subject terminates, the runtime stops
//              the caller (a StopSelf subscription). No wasm callback.
//   - monitor: watching. The subject's terminal event is delivered to the
//              caller's `handle-actor-event` export (a DeliverToWasm
//              subscription).
//
// `link` and `monitor` both key on any termination. `monitor-filtered` lets
// the caller supply its own structural (packr_abi::Pattern) filter instead,
// e.g. `Pattern::any()` to receive every chain event of the subject.
//
// link / monitor / monitor-filtered return `err` when the subscription can't
// be made: "no such actor", or "already terminated: <cause>" when the
// subject's terminal event was already emitted. On `ok`, the subject's
// terminal event will be seen.

interface lifecycle {
    @package: string = "theater:simple"

    exports {
        // Fate-link the caller to `subject` (an actor id). When `subject`
        // terminates, the caller is stopped by the runtime.
        link: func(subject: string) -> result<_, string>

        // Remove the caller's fate-link to `subject`.
        unlink: func(subject: string) -> result<_, string>

        // Monitor `subject` for termination: its terminal event (any cause) is
        // delivered once to the caller's `handle-actor-event` export. For other
        // chain events, use `monitor-filtered`.
        monitor: func(subject: string) -> result<_, string>

        // Monitor `subject` with a caller-supplied filter (e.g. lifecycle-only,
        // or `Pattern::any()` for the whole chain). A broad filter makes the
        // watcher's wasm run for each of the subject's events, so keep it as
        // narrow as the watcher needs. `filter` is a serialized `packr_abi::Pattern` (its
        // `From`/`TryFrom<Value>`) matched host-side against the subject's
        // decoded ChainEventPayload. Delivery is otherwise identical to
        // `monitor`: Target::DeliverToWasm, the same `handle-actor-event`
        // callback.
        monitor-filtered: func(subject: string, filter: value) -> result<_, string>

        // Remove the caller's monitor of `subject`.
        unmonitor: func(subject: string) -> result<_, string>
    }
}

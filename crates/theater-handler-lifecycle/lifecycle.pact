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
// v1 filters are fixed: link keys on any termination, monitor on any lifecycle
// event. Custom structural (packr_abi::Pattern) filters are a forward addition.

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

        // Remove the caller's monitor of `subject`.
        unmonitor: func(subject: string) -> result<_, string>
    }
}

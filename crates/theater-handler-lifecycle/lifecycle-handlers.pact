// Theater Lifecycle Callback Interface
//
// The EXPORT side of monitoring: the function a monitoring actor implements and
// the lifecycle handler calls to deliver a monitored actor's events. The
// handler filters host-side (in Rust) before calling, so the actor is only
// woken for events it asked to watch.

interface lifecycle-handlers {
    @package: string = "theater:simple"

    exports {
        // A lifecycle/chain event of a monitored actor. `subject` is the
        // monitored actor's id, `event-type` its chain event-type string, and
        // `data` the pack-encoded ChainEventPayload.
        handle-lifecycle-event: func(subject: string, event-type: string, data: list<u8>) -> result<_, string>
    }
}

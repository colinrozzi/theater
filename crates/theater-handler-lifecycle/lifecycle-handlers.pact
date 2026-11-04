// Theater Lifecycle Callback Interface
//
// The EXPORT side of monitoring: the function a monitoring actor implements and
// the lifecycle handler calls to deliver a monitored actor's chain events. A
// bare `monitor` delivers only the subject's terminal event; `monitor-filtered`
// delivers whatever its filter matches, which can be ARBITRARY chain events (not
// just lifecycle events). Matching happens host-side (in Rust) before calling,
// so a watcher is only woken for the events it asked to watch.

interface lifecycle-handlers {
    @package: string = "theater:simple"

    exports {
        // A chain event of a monitored actor. `subject` is the monitored
        // actor's id, `event-type` its chain event-type string, and `data` the
        // pack-encoded ChainEventPayload.
        handle-actor-event: func(subject: string, event-type: string, data: list<u8>) -> result<_, string>
    }
}

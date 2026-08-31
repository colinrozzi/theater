// Theater Supervisor Callback Interface
//
// The EXPORT side of supervision: functions the ACTOR implements and the host
// (supervisor handler) calls to deliver events about actors in the actor's
// view. Formalizing this here — it used to be string-only and unhashed — makes
// host and actor agree on one hashed contract, so drift becomes a hash mismatch
// instead of a silent runtime miss.

interface supervisor-handlers {
    @package: string = "theater:simple"

    exports {
        // A chain event recorded by a watched actor (via subscribe-to-actor).
        // Non-terminal activity only — a child's termination arrives via
        // handle-lifecycle-event.
        handle-actor-event: func(id: string, event-type: string, data: list<u8>) -> result<_, string>

        // A child terminated. The supervisor auto-monitors every actor it spawns,
        // so this fires once, automatically, when a child dies — the single death
        // callback (it replaced the old error/exit/external-stop trio). `data` is
        // the child's terminal chain-event payload (decode for the cause + final
        // state). Non-child actors reach this too if explicitly subscribed.
        handle-lifecycle-event: func(id: string, event-type: string, data: list<u8>) -> result<_, string>
    }
}

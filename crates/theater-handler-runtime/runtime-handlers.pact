// Theater Runtime Callback Interface
//
// The EXPORT side of the runtime interface: the ACTOR implements this and the
// runtime calls it to deliver population events (from subscribe-to-spawns).

interface runtime-handlers {
    @package: string = "theater:simple"

    exports {
        // A new actor was spawned anywhere in the runtime. `parent-id` is its
        // spawning supervisor (`none` for a root actor). The observer typically
        // reacts by calling supervisor.subscribe-to-actor(id) to follow it.
        handle-actor-spawn: func(id: string, name: string, parent-id: option<string>) -> result<_, string>
    }
}

// Theater Runtime Interface
//
// The thin SYSTEM-level interface: operate on / observe the runtime as a WHOLE,
// not individual actors (that is supervisor's job — see theater:simple/supervisor).
// Of the entire runtime command surface, only these are truly system-level.
//
// Capability-gated by RuntimePermissions { inspect, mutate }: inspect = observe
// spawns; mutate = shut the system down.

interface runtime {
    @package: string = "theater:simple"

    // Fully enumerable — every op is argument-less and does exactly one
    // fallible thing beyond the permission check (send a command to the
    // runtime), so there is no invalid-argument and no open-ended tail.
    variant runtime-error {
        permission-denied(string),   // required inspect/mutate not granted
        runtime-unavailable,         // the runtime is shutting down / not accepting commands
    }

    exports {
        // Shut down the entire runtime (every actor). Mutate.
        shutdown-runtime: func() -> result<_, runtime-error>

        // Observe the runtime's actor population: after this call, every actor
        // SPAWNED anywhere in the runtime is delivered to this actor's
        // `handle-actor-spawn` export. Births only — a death arrives as the
        // terminal event of a per-actor subscribe (supervisor.subscribe-to-actor).
        // So a global observer's pattern is: watch spawns here, subscribe to each
        // new actor, seal on its terminal event. Inspect.
        subscribe-to-spawns: func() -> result<_, runtime-error>

        // Stop receiving spawn notifications. Idempotent.
        unsubscribe-from-spawns: func() -> result<_, runtime-error>
    }
}

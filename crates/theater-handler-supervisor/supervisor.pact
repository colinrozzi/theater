// Theater Supervisor Interface
//
// Child actor spawning and management capabilities.
// Note: chain-event is approximated as list<u8> for interface hashing.

interface supervisor {
    @package: string = "theater:simple"

    exports {
        // Spawn a child actor (setup + init).
        // The runtime sets up the child and immediately calls its
        // `theater:simple/actor.init` export; the returned id is only
        // valid once init has completed.
        //
        // manifest: manifest path or reference
        // init-state: initial state passed to the child's init.
        //   - `none` means "use the child's manifest.initial_state" — the
        //     supervisor reads the manifest and supplies it as the actor's
        //     state. This is what generic supervisors want when they don't
        //     know the child's secrets.
        //   - `some(v)` means "use exactly v" — even `some(option<...>::none)`
        //     is an explicit override that suppresses the manifest fallback.
        // wasm-bytes: optional WASM bytes (if not provided, loaded from manifest)
        spawn: func(manifest: string, init-state: option<value>, wasm-bytes: option<list<u8>>) -> result<string, string>

        // Spawn a child actor (setup + init) and wait for it to complete.
        // Same `init-state` semantics as `spawn`.
        //
        // timeout-ms: optional timeout in milliseconds
        spawn-and-wait: func(manifest: string, init-state: option<value>, wasm-bytes: option<list<u8>>, timeout-ms: option<u64>) -> result<option<list<u8>>, string>

        // Resume an actor from saved state
        // state-bytes: optional state to resume from
        // wasm-bytes: optional WASM bytes (if not provided, loaded from manifest)
        resume: func(manifest: string, state-bytes: option<list<u8>>, wasm-bytes: option<list<u8>>) -> result<string, string>

        // List all child actor IDs
        list-children: func() -> result<list<string>, string>

        // Restart a child actor
        restart-child: func(child-id: string) -> result<_, string>

        // Stop a child actor
        stop-child: func(child-id: string) -> result<_, string>

        // Get a child's current state
        get-child-state: func(child-id: string) -> result<option<list<u8>>, string>

        // Subscribe to a child's chain events.
        //
        // After this call, every event the named child records to its
        // chain is dispatched to this supervisor's `handle-child-event`.
        // Subscriptions are opt-in: a freshly-spawned child sends no
        // chain events to its parent until the parent subscribes.
        //
        // Idempotent — subscribing a child that is already subscribed
        // is a no-op. Returns an error if the child id is unknown to
        // the runtime (e.g., it has already exited).
        subscribe-to-child: func(child-id: string) -> result<_, string>

        // Stop receiving chain events from a child.
        //
        // After this call, `handle-child-event` no longer fires for this
        // child. Lifecycle handlers (`handle-child-error`, `handle-child-exit`,
        // `handle-child-external-stop`) are unaffected — they ride a
        // separate always-on channel.
        //
        // Idempotent — unsubscribing a child that is not subscribed
        // is a no-op. Subscriptions are also auto-released when the
        // child exits; explicit unsubscribe is only needed when the
        // parent wants to stop observing a still-running child.
        unsubscribe-from-child: func(child-id: string) -> result<_, string>
    }
}

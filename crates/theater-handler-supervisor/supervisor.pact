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
        // init-state: initial state passed to the child's init. The
        //   supervisor host function passes this value through verbatim —
        //   it does NOT fall back to the manifest's `initial_state` field.
        //   Use the conventional `option<list<u8>>::none` sentinel for
        //   "no state".
        // wasm-bytes: optional WASM bytes (if not provided, loaded from manifest)
        spawn: func(manifest: string, init-state: value, wasm-bytes: option<list<u8>>) -> result<string, string>

        // Spawn a child actor (setup + init) and wait for it to complete.
        // Same semantics as `spawn` for the init half.
        //
        // timeout-ms: optional timeout in milliseconds
        spawn-and-wait: func(manifest: string, init-state: value, wasm-bytes: option<list<u8>>, timeout-ms: option<u64>) -> result<option<list<u8>>, string>

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

        // Get a child's event chain
        // Note: chain-event is approximated as list<u8> for interface hashing
        get-child-events: func(child-id: string) -> result<list<list<u8>>, string>
    }
}

// Theater Runtime Control Interface
//
// The runtime-wide CONTROL plane: inspect and drive ANY actor by id
// ("root in Linux"). This replaces the deleted theater-server management
// socket -- a console/sentinel actor imports this interface to BE the server,
// recording every control op in its own chain (the audit log the socket never
// had).
//
// Capability-gated by RuntimePermissions { inspect, mutate } -- the first real
// runtime-side permission enforcement. inspect = the read/subscribe ops below;
// mutate = the lifecycle ops. Only an actor granted `runtime` may call these.

interface runtime {
    @package: string = "theater:simple"

    // One row per live actor. `parent-id` is the spawning supervisor
    // (`none` for root actors) so consumers can render the supervision tree.
    record actor-info {
        id: string,
        name: string,
        parent-id: option<string>,
    }

    exports {
        // --- INSPECT (requires RuntimePermissions.inspect) ---

        // Every live actor, as (id, name, parent-id).
        list-actors: func() -> result<list<actor-info>, string>

        // Live single-actor reads (err if the actor is no longer running).
        get-actor-status: func(id: string) -> result<string, string>
        get-actor-state: func(id: string) -> result<option<list<u8>>, string>
        get-actor-manifest: func(id: string) -> result<string, string>
        get-actor-metrics: func(id: string) -> result<string, string>

        // Subscribe to an actor's chain events. Every event the named actor
        // records is then dispatched to THIS actor's `handle-actor-event`
        // export (mirrors supervisor `handle-child-event`). A terminal event
        // (shutdown / error) is always delivered before the actor's chain
        // closes -- a recorder's seal-and-persist signal. History is the
        // subscriber's job; the runtime retains no chain.
        subscribe-to-actor: func(id: string) -> result<_, string>
        unsubscribe-from-actor: func(id: string) -> result<_, string>

        // --- MUTATE (requires RuntimePermissions.mutate) ---

        // Spawn a new actor from a manifest (setup + init); returns its id.
        spawn: func(manifest: string, init-state: option<value>, wasm-bytes: option<list<u8>>) -> result<string, string>

        // Resume an actor: state is rebuilt by REPLAYING the recorded chain
        // via the manifest's replay handler (not a state blob). Returns the id.
        resume: func(manifest: string, wasm-bytes: option<list<u8>>) -> result<string, string>

        // Single-actor lifecycle control by id.
        stop-actor: func(id: string) -> result<_, string>
        kill-actor: func(id: string) -> result<_, string>
        restart-actor: func(id: string) -> result<_, string>
    }
}

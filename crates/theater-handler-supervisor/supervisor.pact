// Theater Supervisor Interface
//
// Actor-management: operate on the actors in the caller's VIEW. The view is a
// permission — a plain supervisor sees its own subtree (its descendants); a
// control actor is granted `scope: all` and sees every actor. Same ops either
// way; the grant sets the horizon, and every op is evaluated against it (a
// target outside the caller's view is rejected with `out-of-view`). "Control
// actor" is not a separate thing — it's a supervisor with a wider scope.
//
// Note: chain-event is approximated as list<u8> for interface hashing.

interface supervisor {
    @package: string = "theater:simple"

    // One row per actor in the caller's view. `parent-id` is the spawning
    // supervisor (`none` for root actors) so consumers can render the tree.
    record actor-info {
        id: string,
        name: string,
        parent-id: option<string>,
    }

    // An actor id (an opaque string handle). Named for clarity; still a string
    // on the wire.
    type actor-id = string;

    // How any op on this interface can fail. Structured so callers can react
    // (escalate vs give up vs retry) instead of substring-matching. `internal`
    // is the LAST-resort catch-all — an opaque runtime op error we can't yet
    // classify because it crosses the command boundary as a string (the
    // structured-runtime-errors follow-up replaces it with surfaced runtime
    // failures).
    variant supervisor-error {
        actor-not-found(string),    // id is not a live actor (only revealed at scope=all)
        out-of-view(string),        // target is outside the caller's scope
        permission-denied(string),  // required inspect/mutate not granted
        invalid-argument(string),   // bad id / manifest / etc.
        spawn-failed(string),       // spawn or init failure detail
        runtime-unavailable,        // the runtime is shutting down / not accepting commands
        internal(string),           // opaque runtime op error, not yet structured
    }

    exports {
        // Spawn an actor (setup + init). The runtime sets it up and immediately
        // calls its `theater:simple/actor.init` export; the returned id is only
        // valid once init completes. The new actor is a child of the caller.
        //
        // init-state:
        //   - `none`   -> use the actor's `manifest.initial_state`.
        //   - `some(v)` -> use exactly v (even `some(none)` is an explicit
        //                  override that suppresses the manifest fallback).
        // wasm-bytes: optional; if absent, loaded from manifest.package.
        spawn: func(manifest: string, init-state: option<value>, wasm-bytes: option<list<u8>>) -> result<string, supervisor-error>

        // Spawn (setup + init) and block until the actor's init completes.
        // Same `init-state` semantics as `spawn`. timeout-ms: optional.
        spawn-and-wait: func(manifest: string, init-state: option<value>, wasm-bytes: option<list<u8>>, timeout-ms: option<u64>) -> result<option<list<u8>>, supervisor-error>

        // Every actor in the caller's view, as (id, name, parent-id).
        list-actors: func() -> result<list<actor-info>, supervisor-error>

        // Live single-actor reads (err if outside view, or the actor is gone).
        get-actor-status: func(id: actor-id) -> result<string, supervisor-error>
        get-actor-state: func(id: actor-id) -> result<option<list<u8>>, supervisor-error>
        get-actor-manifest: func(id: actor-id) -> result<string, supervisor-error>
        get-actor-metrics: func(id: actor-id) -> result<string, supervisor-error>

        // Lifecycle control of one actor in view.
        stop-actor: func(id: actor-id) -> result<_, supervisor-error>
        kill-actor: func(id: actor-id) -> result<_, supervisor-error>

        // Subscribe to an actor's chain events. After this call, every event
        // that actor records is dispatched to this actor's `handle-actor-event`
        // export. Opt-in and idempotent. A terminal event (shutdown/error) is
        // always delivered before the subscription closes.
        subscribe-to-actor: func(id: actor-id) -> result<_, supervisor-error>

        // Stop receiving chain events from an actor. Idempotent; subscriptions
        // are also auto-released when the actor exits.
        unsubscribe-from-actor: func(id: actor-id) -> result<_, supervisor-error>
    }
}

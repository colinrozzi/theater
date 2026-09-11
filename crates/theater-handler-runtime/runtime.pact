// Theater Runtime Interface
//
// The runtime CONTROL surface: spawn, inspect, and drive actors, plus
// system-wide control (shut the runtime down, observe the spawn population).
// Actor lifecycle is a runtime primitive — there is no separate supervisor
// interface and no view scope; the runtime is flat and every op names its
// target actor by id. (Tracking subtrees is a userland-library concern layered
// on top of `lifecycle` monitors.)
//
// Capability-gated by RuntimePermissions { inspect, mutate }: mutate = spawn /
// stop / kill / shutdown; inspect = list / get-* / observe.
//
// Note: chain-event is approximated as list<u8> for interface hashing.

interface runtime {
    @package: string = "theater:simple"

    // One row per actor in the runtime. `parent-id` is the spawning actor
    // (`none` for root actors) so consumers can render the tree.
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
    // classify because it crosses the command boundary as a string.
    variant runtime-error {
        permission-denied(string),  // required inspect/mutate not granted
        runtime-unavailable,        // the runtime is shutting down / not accepting commands
        actor-not-found(string),    // id is not a live actor
        invalid-argument(string),   // bad id / manifest / etc.
        spawn-failed(spawn-failure),// a spawn/spawn-and-wait failed; see spawn-failure for why
        internal(string),           // genuinely host-internal failure (runtime bug/invariant)
    }

    // Why a spawn failed. Every distinguishable cause gets its own case so the
    // calling actor can react (retry / give up / report) instead of
    // substring-matching one opaque string.
    variant spawn-failure {
        bad-manifest(string),       // manifest string failed to decode / load / parse
        wasm-fetch(string),         // couldn't fetch/load the actor's wasm bytes
        handler-registry(string),   // building handlers from the manifest failed
        wasm-invalid(string),       // wasm failed to instantiate (bad binary / ABI skew)
        interface-mismatch(string), // an imported interface's hash != the host's
        missing-interface(string),  // no handler provides a required interface (grant?)
        missing-metadata(string),   // actor has no __pack_types — not a valid Pack actor
        init-failed(string),        // the actor's own init export errored or trapped
        child-failed(string),       // (spawn-and-wait) the child errored while waited on
        child-stopped(string),      // (spawn-and-wait) the child was stopped externally
        timeout(string),            // (spawn-and-wait) the child didn't finish in time
        internal(string),           // spawn-time host-internal failure (detail preserved)
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
        // Mutate.
        spawn: func(manifest: string, init-state: option<value>, wasm-bytes: option<list<u8>>) -> result<string, runtime-error>

        // Spawn (setup + init) and block until the actor's init completes.
        // Same `init-state` semantics as `spawn`. timeout-ms: optional. Mutate.
        spawn-and-wait: func(manifest: string, init-state: option<value>, wasm-bytes: option<list<u8>>, timeout-ms: option<u64>) -> result<option<list<u8>>, runtime-error>

        // Every actor in the runtime, as (id, name, parent-id). Inspect.
        list-actors: func() -> result<list<actor-info>, runtime-error>

        // Live single-actor reads (err if the actor is gone). Inspect.
        get-actor-status: func(id: actor-id) -> result<string, runtime-error>
        get-actor-state: func(id: actor-id) -> result<option<list<u8>>, runtime-error>
        get-actor-manifest: func(id: actor-id) -> result<string, runtime-error>

        // Lifecycle control of one actor. Mutate.
        stop-actor: func(id: actor-id) -> result<_, runtime-error>
        kill-actor: func(id: actor-id) -> result<_, runtime-error>

        // Shut down the entire runtime (every actor). Mutate.
        shutdown-runtime: func() -> result<_, runtime-error>

        // Observe the runtime's actor population: after this call, every actor
        // SPAWNED anywhere in the runtime is delivered to this actor's
        // `handle-actor-spawn` export. Births only — a death arrives as the
        // terminal event of a per-actor monitor (lifecycle.monitor). Inspect.
        subscribe-to-spawns: func() -> result<_, runtime-error>

        // Stop receiving spawn notifications. Idempotent. Inspect.
        unsubscribe-from-spawns: func() -> result<_, runtime-error>
    }
}

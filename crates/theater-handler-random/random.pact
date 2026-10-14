// Theater Random Interface
//
// Cryptographically-secure randomness for actors, sourced from the host OS
// CSPRNG. This is the COMPLETE random featureset and DELIBERATELY NOTHING MORE:
// no statistical distributions, no user-supplied seeding, no "fast" non-secure
// RNG. Anything above raw secure bytes (a distribution, a shuffle, a seeded
// test PRNG) is built ON TOP in a guest actor, never in the handler.
//
// DETERMINISTIC REPLAY — the important part: every call here is an ordinary
// recorded host-function call. The Pack CallInterceptor records each call's
// input + output to the actor's chain automatically (no handler code needed),
// so on REPLAY the exact same bytes are returned from the chain rather than
// drawn fresh. Randomness therefore fits theater's chain=truth model like any
// other host call: a first run draws real OS entropy; a replay is fully
// deterministic. No seed is needed or offered — the chain is the determinism.
//
// Why a handler at all: a wasm guest has no entropy source of its own (no
// ambient getrandom), so secure randomness must come from the host. A mesh
// node's Noise ephemeral keys, nonces, jitter, and actor-chosen ids all need
// this.
//
// Permissions (manifest [[handler]] type = "random"): max-bytes caps a single
// random-bytes call (anti-DoS / anti-accidental-huge-alloc); unset = a sane
// default (1 MiB).

interface random {
    @package: string = "theater:simple"

    exports {
        // `len` cryptographically-secure random bytes. Errors if `len` exceeds
        // the handler's configured max-bytes cap.
        random-bytes: func(len: u32) -> result<list<u8>, string>

        // One uniform cryptographically-secure u64.
        random-u64: func() -> u64

        // A uniform integer in [min, max) — max EXCLUSIVE. Unbiased (rejection
        // sampling, not modulo). Errors if min >= max.
        random-range: func(min: u64, max: u64) -> result<u64, string>

        // A random v4 UUID, hyphenated lowercase (e.g. an actor-chosen id).
        uuid: func() -> string
    }
}

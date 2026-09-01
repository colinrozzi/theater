# In-module actor state — Design

**Status:** proposal (design agreed; not yet implemented)
**Date:** 2026-09-01

## Summary

Move an actor's state **out of the runtime call path and into the wasm module
itself.** Today the runtime holds each actor's state as a `Value`, threads it in
as the first argument of every export, takes the new state back as the first
return, stores it, and *also* snapshots it into the chain on every wasm call.
Under this proposal the module owns its state privately; only function **args and
return values** cross the boundary. State is no longer a thing the runtime
stores or serializes — it becomes a **replayable projection** of the actor's
input/output history.

The mental model this commits to:

> **The chain (the hash-linked log of inputs and outputs) is the single source
> of truth. State is a private, in-module thing that can always be reconstructed
> by deterministic replay. It is never stored by the runtime.**

## Motivation

State-threading is a remnant of an Erlang-style ambition — hot-swapping
components and migrating live actors between nodes — that carried the state
outside the module so it could be moved or swapped independently. We are **not
pursuing that workflow.** With that goal gone, threading state through every call
is pure overhead, and it is paid **three** times:

1. serialize the state **in** on every call,
2. serialize the new state **out** on every call,
3. write a **full copy of the state into the chain** on every wasm call
   (`WasmEventData::WasmResult { function_name, state, response }`).

For a large-state actor (a chat room, a canvas, a big index) #3 is especially
punishing: the chain — the thing we hash, persist, and replay — bloats with a
complete state copy per event. The only serialization we actually *need* is of
the function call and its return, which we already do to run the system.

## What we give up, and why each is fine

The external-state cache buys four properties. Exactly one of them is something
we want to keep, and it survives.

- **Migration / hot-swap** — the original reason for external state. Explicitly
  abandoned. Not a loss.
- **Cheap crash-restart *with* state** — the runtime re-injects the last-good
  state after a crash. This is a **footgun, not a feature**: whatever poisoned
  the actor is very likely encoded in the state, so restoring it faithfully
  reproduces the crash — a crash-loop on a poison pill. "Let it crash" restarts
  supervised children *clean* for exactly this reason. Removing it is correct. If
  you genuinely want a recovered actor, replay the chain to a **known-good**
  event — never the crash-adjacent state.
- **Replay integrity checking** — today replay can compare reconstructed state
  against the recorded snapshot at each step. We keep this: wasm is
  deterministic, so if replayed **outputs** equal recorded outputs, the state
  transition that produced them is implied equal. Output-equality *is*
  state-equality for a deterministic module. We verify through outputs instead of
  a stored state blob.
- **Universal observability** — the runtime always holds the state, so
  `get-actor-state` is free and generic. This is the one real property, and it is
  preserved (see below), just **computed instead of stored.**

## Observability: opaque by default, `get-state` as an optional export

State is fully the actor's own; the runtime requires nothing of it — not
serializability, not even that it be exposed at all.

- **Live inspection is free and needs no replay.** A running actor already holds
  its state in linear memory. `get-actor-state` becomes: *if the actor exports
  `theater:simple/.../get-state`, call it and return the serialized value; else
  return "opaque".* This is exactly the **optional-export-gated-by-`has_export`**
  pattern the supervisor/lifecycle handlers already use (`handle-lifecycle-event`,
  `handle-actor-event`). No new mechanism.
- **Historical / time-travel inspection** ("what was the state at event 400?")
  becomes: replay the chain to event 400, then call `get-state` on the replayed
  instance — recovering exactly what the in-chain snapshot would have given you.
  O(replay), which is fine for an audit/debug operation.
- **Opaque actors are allowed by choice.** An actor that holds non-serializable
  state (handles, resources) or simply doesn't want to expose itself just doesn't
  export `get-state`. It is inert to inspection. Author's call.

### `theater:simple/actor.get-state` — the export the runtime calls

`get-state` lives **next to `init` in `theater:simple/actor`** — the actor's own
*export* interface (the runtime calls it), not `theater:simple/self` (which the
actor *imports* — `log`/`shutdown`). Same direction as `init`, so it belongs
there:

```
// theater:simple/actor
init:      func(...)          // runtime calls on spawn
get-state: func() -> value    // OPTIONAL; runtime calls it for get-actor-state
```

It stays genuinely optional because **pact exports are declared per-function**,
not per-interface: an actor lists exactly the `interface.function` symbols it
exports (`hello` exports only `actor.init`; `counter` adds
`message-server-client.handle-send`). So an actor includes `actor.get-state` only
if it wants to be inspectable, and the runtime's `has_export` gate probes that one
symbol — absence *is* the "opaque" signal. `get-state` returns a bare `value`
(the state serialized); "can't serialize" is expressed by not exporting it, so no
`option`/`result` wrapper is needed for that case.

### `#[derive(State)]` — the blessed opt-in (a Theater guest macro)

`#[derive(State)]` is a **Theater** concern, not a packr one — "an actor holds
state and optionally exposes it as `get-state` on `theater:simple`" is Theater's
domain model. It lives in the new **`theater-guest`** crate (`crates/theater-guest`
+ `crates/theater-guest-macros`), which re-exports `packr-guest` and adds Theater
ergonomics on top. This **largely decouples the change from pack-dev**: the derive
needs only (a) a module-global cell — [`StateCell<T>`], a plain `static` over an
`UnsafeCell` (sound because actor modules are single-threaded), no packr change —
and (b) to emit the `get-state` export, which reuses packr-guest's *existing*
`#[export]` macro (it supports the zero-param `func() -> value` shape directly).

**Built and validated** (this brick): `#[derive(State)]` on a type emits the cell,
the accessors (`set`/`is_set`/`with`/`with_mut`), and the `actor.get-state` export;
a scratch actor compiles to wasm with the `theater:simple/actor.get-state` symbol
present. One rough edge for the fleet: an actor that derives packr's `GraphValue`
(to get `Into<Value>` for `get-state`) must add `packr-abi` with
`default-features = false` — the GraphValue codegen references the `packr_abi`
crate directly rather than packr-guest's re-export. Actor recipe:

```toml
packr-guest = { version = "0.23", features = ["derive"] }
packr-abi   = { version = "0.23", default-features = false }
theater-guest = "0.1"
```

*(Flag to pack-dev: ideally guest `GraphValue` would target packr-guest's
re-export so this extra dep isn't needed.)*

[`StateCell<T>`]: ../crates/theater-guest/src/lib.rs

Using it gives you a managed, serializable state cell plus an auto-generated
`get-state()` (so the actor stays inspectable and replay-verifiable). Skipping it
lets you hold whatever you want, opaquely.

## The ABI, before and after

Today (`counter`):

```rust
#[export] fn init(_input: Value) -> Value { register(); ok_state(state_with_count(0)) }
#[export] fn handle_send(input: Value) -> Value {
    let s = unwrap_state(input);              // state threaded in
    ok_state(state_with_count(count_of(&s) + 1))   // and back out
}
```

After:

```rust
#[derive(State)]                 // theater-guest: a state cell + auto get-state()
struct Counter { count: u64 }

#[export] fn init() {
    register();
    Counter::set(Counter { count: 0 });       // set once
}
#[export] fn handle_send(from: String, msg: Vec<u8>) {
    Counter::with_mut(|c| c.count += 1);      // mutate in place; nothing threaded
}
```

Every export drops the leading `state` param and the state return. The actor sets
its state once in `init` and mutates it directly.

## Decisions locked

- **State is not required to be serializable.** Author's choice; `#[derive(State)]`
  is opt-in for those who want the managed/inspectable path.
- **`#[derive(State)]` is a Theater guest macro** (in `theater-guest`), built on
  packr-guest primitives — not a packr change.
- **One root state cell** per actor (`#[derive(State)]` on one type is the whole
  state) — not multiple named cells.
- **`init` must set the state**; no magic `Default`. `get-state` before `init` is
  an honest error.
- **Crashes restart clean** (from `init` or a supervisor decision), never a
  state re-inject.

## Runtime changes (theater side)

- **Call path** (`pack_bridge` flattening, `actor/runtime.rs`): stop prepending
  `state` to the args and stop taking `new_state` from the return. `ActorStore`'s
  `state: Value` / `get_state` / `set_state` go away.
- **Chain schema**: drop the `state` field from `WasmEventData::WasmResult` — the
  chain records only call I/O. (Format change; chain events shrink.)
- **`GetActorState` / `get-actor-state`**: call the actor's `get-state` export if
  present (`has_export`), else return opaque/none. Replaces reading held state.
- **Replay**: unchanged in principle — re-run recorded calls; state rebuilds by
  deterministic re-execution. Verify recorded outputs (not a state blob).

## Migration

A fleet-wide ABI break, same shape as the packr-0.23 wave, but **more
self-serve** since the guest ergonomics are Theater's own:

1. ✅ **Done** — `theater-guest` crate (`StateCell<T>`) + `theater-guest-macros`
   (`#[derive(State)]` → cell + accessors + `actor.get-state` export), on top of
   packr-guest. Validated to wasm.
2. Runtime: drop state-threading from the call path, drop `WasmResult.state`,
   re-home `get-actor-state` onto the optional `get-state` export. **Big-bang
   brick** — the ABI break can't land incrementally across the boundary, so the
   runtime flip + the example/test-actor migration ship together.
3. Migrate the canonical examples + test-actors to the new ABI (they're the
   reference; do them first, they gate the rest — and they become the CI guardrail
   that keeps `#[derive(State)]` compiling).
4. Fleet actors migrate one wave.

Coordination with pack-dev is only needed if step 1 surfaces a missing
packr-guest primitive. (Step 1 surfaced one minor rough edge — guest `GraphValue`
needing a direct `packr-abi` dep — noted above; a nice-to-have, not a blocker.)

## Open questions

- ~~Exact interface/name for `get-state`.~~ **Resolved: `theater:simple/actor.get-state`,
  `func() -> value`** — next to `init` in the actor's export interface (see above).
- Whether to keep an **optional replay accelerator** — the runtime calling
  `get-state` every N events and recording a checkpoint, so restore doesn't always
  mean full replay. Pure-replay is the clean default; a checkpoint is an opt-in
  optimization we can add later without changing the model.

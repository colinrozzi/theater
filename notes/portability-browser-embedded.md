# Theater portability: browser + embedded (ideation note)

**Date:** 2026-08-27
**Status:** ideation / feasibility trace — no code changes made
**Origin:** "Could theater nodes run client-side in a browser (Linear-sync-engine
style), and mesh be the sync layer?" → generalized to "abstract theater over the
wasm execution engine," which also unlocks embedded.

## TL;DR

The whole idea holds together. We traced it from vision down to packr's byte-level
calling convention and found **no wall** — only bounded, owned, parallelizable work
plus **one genuine design decision** at the very bottom (browser async host I/O).

Two independent axes:
1. **Concurrency** (in `theater`): detach from tokio → thread-per-actor. Tractable.
2. **Engine** (in `packr` / `../pack`): abstract wasmtime → wasmi (embedded) / JS (browser).
   Contained, and it's our own crate.

They're orthogonal — different crates, only meeting point is `pack_bridge.rs`, which
is already 100% packr-mediated (zero real wasmtime). Can proceed in parallel.

**Embedded is the *easy* target** (every layer tractable). **The browser is where all
the genuine unknowns cluster** — and even those turned out to be charted.

## Axis 1 — Concurrency (theater ← tokio)

- Raw tokio touchpoints ~468, but that conflates cheap and expensive:
  - 95 `async fn`, 203 `.await` (the real grind — reseat on a pluggable executor)
  - **Only 7** tokio channel types in *public* signatures (the API "leaks" — small)
  - `tokio::net`/`std::net`/`std::thread` in core: **0** (all OS I/O already lives in
    the 12 `theater-handler-*` crates, not the core — the sans-io boundary exists *for I/O*)
  - 34 `SystemTime`/`Instant::now` (no_std has no clock → host time capability; distribution
    across metrics-vs-eventchain not yet checked)
- **The `+ Send` bound on `Handler::run` is inherited, not needed.** Handlers capture only
  `Arc`/channel plumbing; nothing is semantically cross-thread. `Send` is load-bearing only
  because of the default multi-thread `#[tokio::main]` + `tokio::spawn`.
  - Relaxing the bound is **backward-compatible** → the 12 handler crates keep compiling.
  - Real edit is core-local: ~8 `tokio::spawn` sites → `spawn_local`/`LocalSet` or thread-per-actor.
- **Thread-per-actor holds cleanly.** Topology is a 2-level tree (TheaterRuntime → ActorRuntime
  per actor → handler/operation/info loops), not a flat mesh. ALL inter-actor + actor↔runtime
  coupling is via channels, never shared memory. So: futures can be `!Send` (pinned per actor);
  channel *messages* stay `Send` (already true). That split IS the actor model's invariant.
  - `theater_runtime.rs:911` = the one real "new actor thread" spawn.
  - `:557/:999/:1065` = detached "send a message" helpers → `spawn_local` on runtime thread.
  - `actor/runtime.rs:551/620/677/691` = actor-local loops → `spawn_local` on that actor's thread.
- Bonus: thread-per-actor is *more* faithful to the actor model AND is exactly what the browser
  (Worker-per-actor) and embassy (task-per-actor) want. The three targets converge on one shape.
  The `+ Send` line is the single thing forcing work-stealing; work-stealing is the single thing
  preventing all three targets from sharing one runtime architecture.

## Axis 2 — Engine (packr ← wasmtime)

- `pack_bridge.rs` imports 16 things from `packr`, only 3 from `wasmtime` — and **all 3 wasmtime
  refs are comments.** Theater is already fully insulated from the engine.
- `packr` is **our own crate** at `../pack` (not upstream/unowned). Biggest scoping relief.
- The ABI is a **separate `no_std` crate** (`pack-abi`: `Value`/encode/decode, zero wasmtime).
  The host↔guest contract is already engine-independent — the hard seam already exists.
- wasmtime lock is **one module, ~3,400 lines**: `src/runtime/mod.rs` + `host.rs` (+ small checker).
  Only **2** wasmtime types leak publicly (`pub use wasmtime::{Engine, Module}`); everything theater
  consumes (`AsyncRuntime`, `Ctx`, `PackInstance`, `HostLinkerBuilder`) is a packr-owned wrapper.
  The abstraction is de-facto present, just concrete instead of trait-shaped.
- **wasmi (embedded): low effort.** wasmi deliberately mirrors wasmtime's API
  (`Engine`/`Module`/`Store`/`Linker`/`Caller`/`Memory`). Fork ~3.4k lines ≈ find-and-adapt.
  wasmi is `no_std` → exactly what embedded needs. Preemption via **fuel** (wasmtime uses epoch).
- **Browser: different in kind** — no Rust engine; drive the browser's `WebAssembly` from JS.

## The browser deep-dive (the last "red box", now charted)

- **Module format:** `0` `wasmtime::component` refs — packr runs on **core wasm** (`Module::new`).
  → browser instantiates natively via `WebAssembly.instantiate`. No jco/transpile. Biggest risk gone.
- **Host fns, two paths:**
  - Sync (`func_wrap`): fixed 32KB host scratch buffer → plain JS import.
  - Async (`func_wrap_async`): unbounded returns; the host fn **re-enters the guest's `__pack_alloc`**
    to size the return buffer (host.rs:507). It's "async" only because **wasmtime's Rust API needs an
    async store to re-enter the guest** — a wasmtime ergonomics constraint, NOT a fundamental async op.
  - **In the browser that re-entrancy is a plain synchronous nested call** (JS import → `exports.__pack_alloc`).
    The browser does *for free/synchronously* what wasmtime needs its heaviest machinery for.
- **The one genuine design decision:** some async host fns also do *real* async I/O (`store.get`,
  message-server) before allocating. A JS import can't block on a Promise. Options:
  1. **JSPI** (WebAssembly JS Promise Integration) — purpose-built, shipping in Chrome. Clean answer.
  2. **Asyncify** — universal, older, overhead.
  3. **Message-pass the I/O** — fire-and-return host fn, result arrives as a later operation.
     Actor-model-native; sidesteps sync-await. Fits theater's operation loop.
- **Breadcrumb:** `host.rs:212-217` comments say "underlying **wasmi** Caller" while the code uses
  `wasmtime::Caller` → packr may have a prior wasmi↔wasmtime migration. Check `git log` on host.rs.
- **Preemption converges on browser being the odd one out:** wasmtime=epoch, wasmi=fuel,
  browser=none → `Worker.terminate()`. Same place every hard edge lands.

## Full-vision cost map

| Layer | Native | Embedded | Browser |
|---|---|---|---|
| ABI (`pack-abi`) | done | done (`no_std`) | done |
| Engine (`packr/runtime`) | done | fork ~3.4k → wasmi (API mirrors) | reimplement vs JS/wasm-bindgen |
| Concurrency (`theater`) | done | thread-per-actor + embassy | Worker-per-actor + wasm-bindgen-futures |
| Handlers I/O (12 crates) | done | per-platform (peripherals) | per-platform (fetch/IndexedDB/WebRTC) |
| Preemption | epoch | fuel | Worker.terminate |

## The mesh angle (the original spark)

- Mesh core is already `no_std`, sans-io, transport-agnostic ("sockets are the system's, not the
  core's" — emits `Effect::Send`/`Close`). TCP→WebSocket/WebRTC is a *host binding* change, not a
  core change. The substrate half is essentially browser-ready.
- **Tension:** mesh is CP with all-members finality + no fault tolerance ("a fixed, present set").
  Browser tabs are the opposite. User wants **browsers-as-peers (pure p2p)**, which fights this.
  - Resolution direction: mesh is two layers. Bottom (signed causal DAG + grafting-gossip + ed25519
    identity) is *perfect* for p2p browsers. Top (all-members finality → total order) is the CP part
    that breaks with ephemeral peers. For browsers-as-peers: keep the bottom, move "agreement" from
    finality (CP) to convergence (CRDT over the DAG, keyed on v0.3 signed timestamps). That's closer
    to what Linear actually does than CP consensus is.
  - "No central server" still needs a **dumb signaling relay** for WebRTC (SDP/NAT) — no authority,
    no state. "No authoritative server," not "no server."

## Recommended next steps (when picked up)

1. **De-risk the browser corner with a spike:** instantiate ONE packr component from JS
   (`WebAssembly.instantiate`), call `init`, honor the `__pack_alloc`/`__pack_free` ABI, and prove
   the re-entrant guest-alloc path works as a synchronous nested call. Decide JSPI vs message-pass
   for real async I/O.
2. **Or take the easy win first — embedded/wasmi:** it's the path of least resistance to "theater
   off wasmtime" (wasmi mirrors wasmtime, no_std native, fuel for preemption). Good proof the engine
   trait factors cleanly before tackling the browser's JS host.
3. Formalize `src/runtime/` behind an engine trait (it's ~90% wrapped already); drop the 2
   `pub use wasmtime::` leaks.
4. In theater: relax `Handler::run`'s `+ Send`, convert ~8 spawn sites to thread-per-actor.
5. Housekeeping: theater pins `packr 0.11`, pack is at `0.21` — reconcile version drift first.

## Unresolved / not yet chased

- Where the 34 clock calls cluster (metrics = droppable vs event-chain = load-bearing).
- `messages.rs` (1042 lines) as a possible second channel-leak surface beyond `ActorOperation`.
- `store/mod.rs` (68 touchpoints / 22 async fns — densest tokio file) full per-platform reimpl cost.
- Actual JSPI spike (feasibility reasoned, not demonstrated).

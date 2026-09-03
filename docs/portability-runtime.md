# Portable runtime: an executor-agnostic core + per-environment drivers

**Status:** design (audit complete; no code yet)
**Date:** 2026-09-03
**Origin:** `notes/portability-browser-embedded.md` — "run theater nodes client-side
(browser, Linear-sync style), mesh as the sync layer" → "abstract theater over the
execution substrate," which also unlocks embedded.

## Thesis

Theater's **structure** — the actor model (isolation, sequential message
processing, comms only by channels), the `Handler` contract, the chain,
supervision, the packr engine seam — is environment-independent. What varies is the
**scheduling driver**: *how* actor loops get put on the substrate.

So we split:

- **`theater-core`** — what an actor *is* and how it processes messages.
  Executor-agnostic. Owns **no executor and no clock**.
- **`theater-driver-native`** — the tokio work-stealing driver we run today,
  repackaged as *a* driver. Owns spawning (and its `Send` requirement), time, and
  preemption.
- **`theater-driver-browser` / `theater-driver-embedded`** — peer drivers. The
  payoff.

**Work-stealing is not the thing to escape — it's a good driver.** For a server
with thousands of actors on a few cores, M:N work-stealing is exactly right, and
thread-per-actor would be strictly worse. The move is to stop *baking work-stealing's
requirement (`Send` everywhere) into the shared core*, so it becomes a **choice**
rather than an **assumption**.

This is the same move we've made twice: we sans-io'd the handlers (I/O at the edges)
and the mesh sans-socket'd its core (emits `Effect::Send`, driver owns the socket).
This applies it to **scheduling**: the core stops touching the things that differ
across environments, and the driver drives it.

## The seam: what the core needs from a driver

The audit (below) says the seam is smaller than the note first sketched:

1. **Spawn** — start a long-lived actor/handler loop. Driver-provided; the core is
   *generic over the driver*, so it never states a `Send` bound — the bound comes
   from the concrete driver per build (native requires `Send`, browser doesn't).
2. ~~**Clock**~~ — **dissolves.** Observability timing goes to the log/metrics edge;
   the sleep/epoch watchdogs are *preemption*, which is already per-target.
3. **Channels** — become a non-issue by switching core from tokio channels to
   `futures::channel` (portable: native, wasm, embedded). One gap: `watch`.

## The audit (load-bearing section)

### Spawn sites — 7, and 6 are clean

| # | Site | What it spawns | Kind |
|---|---|---|---|
| 1 | `theater_runtime.rs:766` | `ActorRuntime::start` for a new actor | long-lived actor task → **driver** |
| 2 | `actor/runtime.rs:645` | the actor's setup + main select loop | long-lived → **driver** |
| 3 | `actor/runtime.rs:701` | `info_loop` (ActorInfo requests) | long-lived → **driver** |
| 4 | `actor/runtime.rs:715` | `operation_loop` (function calls) | long-lived → **driver** |
| 5 | `actor/runtime.rs:577` | a handler's `setup`/run loop | long-lived → **driver** (but see the fork) |
| 6 | `replay/handler.rs:245` | replay verification loop | long-lived → **driver** |
| 7 | `theater_runtime.rs:868` | **init-fire** — call `actor.init` off the spawn path | **detached-await helper** → restructure |

Sites 1–6 are "**hand the driver a loop to run**." The driver spawns them however
it likes (work-stealing / Worker / embassy task). Site 7 is the only "detached to
avoid blocking" spawn — and `init` is *already* an operation flowing through the
actor's `operation_loop` (site 4); the spawn only exists to await its result off the
spawn path. It can become: fire the init operation, don't await it here (or await
via a completion channel). **No spawn.**

Net: the core can own **zero** executor. It hands loops to a driver-provided `Spawn`
and never spawns anything itself.

### Clock sites — all observability or preemption

Of the ~22 `now()` calls: **21 are metrics**, plus `phase_start`/`start` elapsed
timing for spawn-bench and phase logging. All **observability** — droppable, or
stamped at the log/metrics sink, not smeared through core logic. The remainder —
3 `sleep`, the epoch `interval`, 1 `watchdog` (`pack_bridge.rs:95/338`,
`theater_runtime.rs` init-watchdog) — are **preemption**: catch a runaway guest.
Preemption is *already* per-target (epoch native / fuel wasmi / `Worker.terminate`
browser — the odd-one-out row in the note's cost map). So it belongs to the
driver/engine, not core.

**Nothing in core's *logic* depends on a monotonic clock.** (One thing to confirm:
chain event `timestamp` — believed informational, since chain order is by
`parent_hash` hash-linking, not time. If so, it's driver-stamped or optional.)

### Channels — futures, with one gap

55 `oneshot` + 33 `mpsc` (tokio) → `futures::channel::{oneshot, mpsc}` — portable,
mechanical swap, and channels stop being a driver concern. The **3 `watch`** (the
`ActorPhaseManager` phase broadcast) are the gap: `futures` has no `watch`. Options:
a tiny portable watch, `async-broadcast`, or restructure the phase signal onto an
mpsc/observable. Small, contained.

## The crux: where `MaybeSend` is irreducible — and the fork that removes it

`MaybeSend` (a conditional `Send` bound) is a *smell*, and the audit says where it
does and doesn't belong.

- The executor, the spawns, channels, and clock **do not** force it — they're
  driver-owned or portable.
- It survives at **exactly one point**: the **`dyn Handler` boxed future**. The open
  handler registry (which we just built) needs `Box<dyn Handler>`; a boxed
  trait-object future must declare its `Send`-ness *at the trait*, and that differs —
  native work-stealing needs `Send`, a browser handler holding JS handles can't be
  `Send`. That one bound is the irreducible tension of {open registry × work-stealing
  × browser}.

**But there's an escape, and it's the same insight the note parked as "browser async
host I/O."** If handlers stop being *self-driven async loops* and become
**synchronous step functions that emit effects** — message-pass I/O: a handler
returns "do this I/O," the driver performs it, the result arrives as a later
*operation* — then:

- Spawn site #5 (the handler loop) **disappears** — handlers don't run loops.
- `Handler` has no `run`/`setup` future → **no boxed future → no `MaybeSend`, anywhere.**
- Browser async I/O is **free**: effects are performed by the driver and delivered as
  operations; no JSPI, no Asyncify. The thing wasmtime needs its heaviest machinery
  for, the browser gets for nothing.

This is the mesh's sans-io model applied to handlers — and it's the "in-module-state
moment": a portability constraint revealing a cleaner model that's better *everywhere*,
not just in the browser.

### The fork (this is the real decision)

- **Path A — keep handlers as async loops.** Least disruptive to the 12 handler
  crates. Cost: one cfg'd `Send` bound survives on the `Handler` future, and the
  browser needs JSPI/Asyncify for handler async I/O.
- **Path B — handlers = sync step + effects (message-pass I/O).** Rewrites the handler
  model (the 12 crates move to the effect shape). Payoff: fully sans-executor core,
  **no `MaybeSend`**, browser async I/O for free, and a handler contract that's more
  actor-native. Bigger, but it's the version where the smell doesn't exist rather than
  gets confined.

My lean: **Path B**, staged — but it's genuinely a bigger commitment, and it's the
one call in here that should be made deliberately, not defaulted.

## Brick sequence (each independently green, native unchanged until the split)

1. **Audit** — done (this doc).
2. **Rip Clock:** move observability timing to the metrics/log sink; leave epoch/
   watchdog where they are (already engine/driver). Confirm the chain timestamp.
   Non-disturbing on native.
3. **Restructure the init-fire** (spawn site #7 → operation + completion channel).
   Kills the one detached spawn. Non-disturbing.
4. **Spawn seam:** introduce a driver-provided `Spawn`; route the 6 loops through it;
   native impl = `tokio::spawn`. Behavior byte-identical on native. *This seam is the
   core/driver boundary.*
5. **Channel swap** to `futures::channel` (+ the watch gap).
6. **Crate split:** extract `theater-core` + `theater-driver-native`. Now the boundary
   is a crate boundary; a new driver is a new crate.
7. **The fork decision (A/B)** — gates the handler model.
8. **`theater-driver-browser`** — Worker-per-actor + `wasm-bindgen-futures`; the engine
   axis (`packr` → JS `WebAssembly`) is pack-dev's parallel track.

## Open questions

- **The A/B fork** (handler model) — the deliberate call.
- **Chain `timestamp`** load-bearingness (believed informational; confirm).
- **The `watch` gap** (phase manager) — portable watch vs restructure.
- **Engine axis** (`packr` ← wasmtime → wasmi/JS) — pack-dev's crate, parallel and
  independent; this doc is the *concurrency* axis only.

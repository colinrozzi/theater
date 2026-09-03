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
2. ~~**Clock**~~ — **ripped out of the core call path** (DONE, PR pending — see brick 2).
   The audit's first framing ("timing moves to a log sink") was half-wrong: *durations*
   are measured start→end *inside* core, so a sink can't recover them. But that timing —
   plus the whole metrics subsystem and the `ChannelId` wall-clock entropy — is **sugar**.
   Colin's call: rip it, don't abstract it (a Clock capability / a `web-time` swap would
   just relocate the coupling). Only the *async timers* (teardown timeouts, init-watchdog,
   epoch ticker) matter; they're preemption/safety and go to the driver at the split.
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

## `MaybeSend` doesn't belong in core — the `Handler` trait does the confusing

`MaybeSend` (a conditional `Send` bound) looks unavoidable only if you assume **one
`Handler` trait, living in core, that must be `Send` for native work-stealing and
`!Send` for a browser handler simultaneously.** That assumption is false.

**Handlers are per-environment.** The browser runs fetch/IndexedDB/WebRTC handlers;
native runs TCP/filesystem/podman; embedded runs GPIO/I2C. These aren't one handler
ported three ways — they're different code. So there is no shared cross-environment
`Handler` trait to protect, and no reason it lives in core.

So: **push the `Handler` contract into the drivers.**

- `theater-driver-native` defines *its* handler contract — `Send`, independent async
  loops, work-stealing. The native handler crates implement that.
- `theater-driver-browser` defines *its* — Worker-spawned, `!Send`, its own loop
  model. Browser handlers implement that.
- `theater-core` never sees `dyn Handler`, never declares `Send`, and **keeps
  handlers as independent async loops** — the loop-ness and the `Send`-ness are the
  *driver's* to decide, and each driver resolves it naturally. **No `MaybeSend`
  anywhere, and the loop model we like is preserved.**

### Two async things, only one forced

The earlier draft conflated two things the audit now separates:

- **(i) Handler *run loops*** (tcp-accept, message-pull — inject events *into* an
  actor). These go **driver-side**, stay loops, per-environment. Solved by the above.
- **(ii) Async host-function *calls*** — the guest, mid-execution, calls a host fn
  (`store.get`, message-server `request`) that does real I/O before returning. This
  one is **core-visible no matter what**, because the host fn re-enters the
  core-owned wasm instance to size its return (`__pack_alloc`). This — not the loops —
  is the only genuinely-forced decision.

And (ii) is **surgical, not a handler rewrite.** Most host functions are synchronous
(`log`, the vast majority) and don't care. Only a handful do real async I/O. So the
one remaining fork is, *for just those*:

- **`await` them** — the operation loop awaits the async host fn. Simple on native;
  needs JSPI/Asyncify on the browser (a JS import can't block on a Promise).
- **fire-and-return** — the host fn kicks off the I/O and returns immediately; the
  result arrives as a later *operation*. No mid-execution await → no `Send` pressure,
  no JSPI, and it's actor-native.

That fork is the subject of the next section (async host calls). Everything else —
Clock removal, the spawn seam, `futures::channel`, the crate split — is unaffected by
it and keeps native byte-identical.

## Async host calls: the capability model + the async-ABI target

A *waiting* host call (`store.get`, message-server `request` — I/O before it can
return) is the one genuinely-forced portability question. There are ~75 of them
(`func_async_result`); most host fns (`log`, …) are synchronous and don't care.

The unlock: **a wasm *call* and an async *task* are different things.** The host
can't pause a wasm call mid-execution (the browser limitation), but a wasm call can
*return normally* while an async task inside it is **parked** — its half-done state
saved in the wasm's linear memory (the same persistence that makes in-module state
work), and resumed by a *later* call.

There are **three layers**, and the async machinery lives in the middle one, not in
the actor's code:

```
WASM ("the guest") = actor logic (author's `store.get(h).await`)
                   + guest runtime lib (packr-guest/theater-guest: allocator, ABI, executor)
HOST (native Rust / browser JS) = runs the wasm, provides host fns, does the I/O
```

So "wait for a slow call" is a **driver capability**, invisible to the actor. The
actor always writes `let c = store.get(h).await;` — one style, everywhere. Drivers
differ in *how* they realize the wait:

| Driver | Wait mechanism | Runs |
|---|---|---|
| native | wasmtime suspends the wasm call | all actors |
| browser (JSPI) | engine suspends the wasm call (needs Chrome-class JSPI) | all actors |
| browser (**async-ABI**) | **guest runtime parks the task; host fn fires-and-returns a ticket; result comes back as a `resume(ticket, …)` call** | all actors — **needs no engine feature** |
| browser (minimal) | none | only actors that never wait |

**Portability principle:** drivers advertise capabilities; actors have needs; an
actor runs on any driver that meets its needs — its source never changes. `.await`
already means "may suspend here"; whether that's realized by the engine (JSPI) or
the guest runtime's executor (async-ABI) is below the author's code.

**Target = the async-ABI tier.** It's the one that runs an actor on *any* platform
with no engine dependency, keeps actor code inline, and drops straight into
theater's operation loop (a result is just another operation). Its cost: the
guest-runtime half (executor + park/resume + ticket→state mapping + the ABI) is
**pack-dev's** area (`packr-guest`), so it's a coordinated change, not theater's
alone.

**Sequencing:** JSPI is a fast interim — proves the whole browser stack (engine +
Worker + core/driver split) while touching almost nothing, and doesn't foreclose the
async-ABI. The `await`-style host fns migrate to fire-and-return incrementally. The
minimal sync-only browser is a near-free floor tier.

## Core handlers vs driver handlers

A few current "handlers" aren't environment I/O at all — `message-server` (inter-actor
comms), `supervisor`, `lifecycle`, `self` are **actor-model** concerns. Those likely
belong in **core**; only the genuine I/O handlers (tcp/filesystem/http/podman) are the
per-environment/driver set. The split is "core handlers vs driver handlers," which
also draws the line between where the actor model ends and the environment begins.

## Brick sequence (each independently green, native unchanged until the split)

1. **Audit** — done (this doc).
2. **Rip Clock (DONE):** delete the observability clock from the core call path —
   phase-timing bench logs (the init-hang-saga scaffolding), chain-dispatch timing,
   pack_bridge compile timing, the whole **metrics subsystem** (incl. the supervisor
   `get-actor-metrics` op — a deliberate fleet ABI change), and the `ChannelId`
   `chrono::Utc::now()` entropy (rand alone suffices). Kept: the async timers (3 teardown
   timeouts, init-watchdog, epoch ticker) — preemption/safety, driver-bound at the split.
   Correction to the audit: there is **no** load-bearing chain-event timestamp; the only
   `chrono` was ChannelId hash entropy. Native behavior identical (lib 35 / doctests 21 /
   theater-tests teardown net all green). +7/−300 + metrics.rs (−250).
3. **Restructure the init-fire** (spawn site #7 → operation + completion channel).
   Kills the one detached spawn. Non-disturbing.
4. **Spawn seam:** introduce a driver-provided `Spawn`; route the 6 loops through it;
   native impl = `tokio::spawn`. Behavior byte-identical on native. *This seam is the
   core/driver boundary.*
5. **Channel swap** to `futures::channel` (+ the watch gap).
6. **Crate split:** extract `theater-core` + `theater-driver-native`, moving the
   `Handler` contract + the I/O handlers driver-side while `message-server`/
   `supervisor`/`lifecycle`/`self` stay core. A new driver is a new crate.
7. **The async-host-call decision** — `await` vs fire-and-return (next section);
   gates the browser driver, not the native path.
8. **`theater-driver-browser`** — Worker-per-actor + `wasm-bindgen-futures`; the engine
   axis (`packr` → JS `WebAssembly`) is pack-dev's parallel track.

## Open questions

- ~~**Async host calls**~~ — **settled** (see the section above): capability tiers,
  actors stay one style, **async-ABI is the target** (portable everywhere, no engine
  feature), **JSPI a fast interim**, sync-only a floor. The async-ABI's guest half is
  a coordinated change with pack-dev (`packr-guest`); not blocking the native reshape.
- **Core-handler vs driver-handler line** — confirm `message-server`/`supervisor`/
  `lifecycle`/`self` are core; tcp/filesystem/http/podman are driver.
- **Chain `timestamp`** load-bearingness (believed informational; confirm).
- **The `watch` gap** (phase manager) — portable watch vs restructure.
- **Engine axis** (`packr` ← wasmtime → wasmi/JS) — pack-dev's crate, parallel and
  independent; this doc is the *concurrency* axis only.

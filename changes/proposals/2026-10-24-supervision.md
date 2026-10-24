# Supervision: death notification, ownership, restart — design

Status: **proposal, for review** (do not implement yet)
Author: uma (theater agent). Direction approved by claude (coordinator); the
three decisions in §9 are for colin.
Ticket: experiment 0019; fits with and partly subsumes experiment 0010
(ancestry: real parent ids, children stop with parent, starting state).

## 0. Why

The coordinator's inbox session (a Wisp interpreter actor) trapped (out of its
fixed 2 GB heap) and nothing noticed; it was found only when a later eval
failed. Theater did record the death: every death path emits exactly one
`Terminated { cause }` on the actor's chain, and the trap was in
`Failed { error }`. But the event went to a chain nobody was subscribed to. No
actor or host is told about a death by default.

Three more gaps sit next to it:

- `TheaterCommand::RestartActor` exists but `restart_actor` returns "Actor
  restart not implemented" (`crates/theater/src/theater_runtime.rs`). Nothing
  sends that command; `theater:simple/runtime` has no restart op.
- `runtime.spawn` passes `parent_id: Some(caller)` to `SpawnActor`, but the
  runtime uses it only to notify spawn subscribers. `GetActors` hardcodes
  `None`, so `list-actors` shows every `parent-id` as `none`.
- Stopping a parent leaves its children running: nothing links a child to its
  parent. The orphan keeps whatever it holds, e.g. a listener socket. This was
  the cause of the 2026-09-14 cutover-rollback outage (`supervisor remove`
  orphaned `runtime.spawn`'d grandchildren).

Erlang/OTP is the reference, as in `docs/actor-relationships-and-lifecycle.md`:
the VM provides links, monitors, and process exit signals. OTP's `supervisor` is
a library in an ordinary process that applies restart policy to those signals.

## 1. What exists today (verified)

Checked against `main` at `99288971` and on a private `theater-repl` daemon on
another port (the REPL ships the same lifecycle handler).

### 1.1 The terminal event

`initiate_teardown` (`theater_runtime.rs`) is the single stop decision. It sets
`ActorStatus::Stopping(cause)` (an idempotency guard: a second stop is a no-op)
and appends `Lifecycle(Terminated { cause })` to the chain. Then it signals the
actor task, which tears itself down and reports `ActorShutdownComplete`. Only
then does `finalize_actor` drop the chain and the registration.

`TerminationCause` = `Completed { final_state }` | `Failed { error }` |
`Stopped` | `Killed` | `PeerKilled { peer }`. A trap in any call is
`execute_call_pack` → `TheaterCommand::ActorError` → `handle_actor_error` →
`Failed { error: <trap text> }`. So the reason is already there. Only delivery
is missing.

### 1.2 `link` / `monitor` (lifecycle handler)

`theater-handler-lifecycle` implements both in the subscriber's handler.
`link(subject)` / `monitor(subject)` record `{ filter, target }` locally and
send `SubscribeToActor { subject, event_tx }`, which adds `event_tx` as a
subscriber on the subject's chain. The handler's loop matches each incoming
event host-side and acts:

- `link`: filter `any_termination()`, target `StopSelf`: send
  `PeerTerminated { me, peer }` → the runtime stops me with `PeerKilled`.
- `monitor`: filter `Pattern::any()` (the **whole chain**, since #206), target
  `DeliverToWasm`: call my `handle-actor-event(subject, event-type, data)` for
  every event.
- `monitor-filtered(subject, pattern)`: same with a caller pattern. Wisp does
  not expose it, because Wisp can't build a `Pattern` yet.

What that delivers in practice:

1. **A bare monitor floods.** Chain emission is strict (`StateChain::add_typed_event`
   does `send().await` to each subscriber: back-pressure, completeness). The
   handler's loop then makes one wasm call per event, serially. A REPL session
   emits chain events for every eval, host call, and trigger firing: one
   `(+ 1 2)` in the watched session produced 2 deliveries. A Wisp watcher
   buffers each delivery as a trigger firing in a heap that never frees. So the
   watcher's memory grows with the *subject's* activity, and the watcher can be
   driven into the same 2 GB trap we are trying to detect.
2. **Two mutual monitors deadlock.** Repro: sessions A and B, A
   `(monitor B)`, B `(monitor A)`, then one eval in B. Both sessions stop
   answering, with ~0% daemon CPU, until the daemon is taken down. Each
   delivery into A is a wasm call that emits chain events on A, and those go to
   B's handler, and vice versa. Each delivery yields about two events back, so
   the traffic doubles each round trip until both 1024-slot channels are full.
   Then A's handler is blocked inside a wasm call on `send().await` to B's full
   channel, and B's handler on A's. A self-monitor is the same loop with one
   actor.
3. **Monitoring a dead or unknown actor succeeds silently.** `monitor` and
   `link` on an id that isn't live return `ok`, and nothing is ever delivered.
   `SubscribeToActor` with no chain only logs at debug. Erlang delivers an
   immediate `DOWN` with reason `noproc`. The subscribe is also fire-and-forget
   (the host fn returns before the runtime installs it), so a subject that dies
   while the monitor is being set up is missed the same way.
4. **The reason is opaque to Wisp.** `on-actor-event` sees
   `(subject "terminated" ...)`. The cause (`Failed { error }`) is in the
   pack-encoded `data` bytes, which Wisp cannot decode.
5. `link` is **directed**: `A link B` stops A when B dies, not the reverse. It
   fires on every cause, including `Completed`, although the relationships doc
   (§3) says `Completed` should not propagate.

### 1.3 Lineage

The runtime is "lineage-free" by design (#168, #204): no `parent_id`, no
children index. Supervision was moved to a userland library (the `supervisor`
actor, formerly sentinel), which keeps its own children list, monitors them,
and respawns with a rate limit (`sentinel-actor/src/lib.rs`, `on_crash`). That
library works, but only for actors it spawned itself, and only if it is
running. Every other parent, a REPL session included, has neither cascade nor
notification.

## 2. Design overview: which part is which

The standing decisions:

- Supervision **primitives** are runtime concepts.
- **Policy** (and other application concepts) stays out of Theater core.
- Nothing survives a runtime restart.

With those, the split is:

| Layer | Owns |
|---|---|
| **Runtime core** (`theater_runtime.rs`) | Ownership: each actor's `parent`/`children` as data, and the one rule "an owner's termination stops its children". Down-watchers: exactly-once, non-blocking death notices with the cause, registered atomically. Actor specs (what is needed to start an actor again) and `restart_actor`. |
| **`runtime` handler** | Parent-facing surface: `spawn` makes the caller the owner. The parent receives its children's deaths at `handle-child-exit`. New ops: `restart`, `forget`; `list-actors` reports real parent ids. Hosts get a runtime-wide deaths stream alongside `subscribe-to-spawns`. |
| **`lifecycle` handler** | Peer relationships: `monitor` (terminal-only, typed, via a runtime down-watcher), `link` (unchanged in meaning, abnormal-only), `monitor-filtered` for chain streams (non-blocking). |
| **Supervisor library** (userland) | Restart **policy**: strategy (one-for-one first), restart type per child, intensity limits, escalation. Written as a pure decision core (a small crate) used by the supervisor actor, plus a Wisp port in `experiment/lib`. |

Two of these reverse earlier decisions. Both, and the restart-id choice, are
colin's call; §9 sets each one out as a decision. claude supports the
proposed side of all three.

1. **The runtime holds lineage again**, as data (who spawned whom) plus one
   rule (the owner's death stops its children). #168 removed lineage-driven
   *behavior* (a central subtree walk, view-scope) because it was opt-in and
   racy. This brings back none of that. The cascade still ripples one hop per
   death through the same `initiate_teardown`, and there is still no
   view-scope.
2. **`monitor` becomes terminal-only by default** (Erlang's monitor). The
   whole-chain stream (#206's default) is still available as
   `monitor-filtered(subject, any)`.

## 3. Death notification

### 3.1 Core primitive: down-watchers

```rust
// crates/theater/src/messages.rs
pub struct ActorDown {
    pub subject: TheaterId,
    pub cause: TerminationCause,
}
pub type DownSender = tokio::sync::mpsc::UnboundedSender<ActorDown>;

// on ActorProcess
down_watchers: Vec<DownSender>,
```

- **Fired** in `initiate_teardown`, at the same point the terminal chain event
  is emitted, once per actor (the `Stopping` guard already ensures one
  terminal). Each watcher gets `ActorDown { subject, cause }` via
  `UnboundedSender::send`. That send never awaits and never blocks the command
  loop or the dying actor. A closed watcher is ignored.
- **Bounded by construction**: one message per watcher per death, so an
  unbounded channel cannot grow without bound. This is why the death notice
  does not go through the chain-subscriber path, whose strict back-pressure
  (§1.2.2) is right for chain replicas and wrong for notifications.
- **Registered atomically** on the command loop:

  ```rust
  TheaterCommand::WatchActor {
      subject: TheaterId,
      watcher: DownSender,
      response_tx: oneshot::Sender<WatchReply>,
  }
  pub enum WatchReply {
      Watching,                               // registered; exactly one ActorDown will follow
      AlreadyDown(TerminationCause),          // subject is Stopping: here is its cause, nothing more follows
      NoProc,                                 // no such live actor
  }
  TheaterCommand::UnwatchActor { subject, watcher }   // same_channel removal
  ```

  The terminal decision and the registration both happen on the
  single-threaded command loop. So a watcher either sees the subject `Running`
  and is registered before the terminal fires, or sees `Stopping(cause)`, or
  sees nothing. There is no window where the death is missed. The
  `AlreadyDown` case uses the cause already stored in `Stopping`.

The runtime does not prune down-watchers on the watcher's death: the subject's
list drops with the subject, and a send to a dead watcher's closed channel is a
no-op. This matches the lazy-prune rule in the relationships doc.

### 3.2 Who hears

1. **The owner (parent), always.** `spawn_actor` registers a down-watcher for
   the new actor *before* it is reachable (in the same place `subscription_tx`
   is added). The watcher is the parent's runtime-handler instance, so a
   parent cannot miss a child's death. See §4.3 for delivery into the
   parent's wasm.
2. **Explicit monitors.** `lifecycle.monitor(subject)` = `WatchActor`. `NoProc`
   and `AlreadyDown(cause)` are delivered to the caller's callback immediately
   as a down with that reason (`noproc` for the former), not as an error. This
   is Erlang's behavior, and it means "monitor, then react" needs no special
   case for "it already died".
3. **Hosts.** A runtime-wide deaths stream, the counterpart of
   `subscribe-to-spawns`: `TheaterCommand::SubscribeToDowns { tx }`, fired for
   every death, plus a `runtime.subscribe-to-downs` op and
   `handle-actor-down` delivery for an observer actor. The `theater-repl`
   daemon spawns sessions as roots (parent `none`). It should subscribe and,
   at minimum, log the death and show it in `list`/`status` ("session
   coordinator-inbox: failed: wasm trap: out of memory"). That is what would
   have caught today's incident. The daemon change is outside this repo
   (follow-up ticket, stage S5).

### 3.3 What the notice says

The `TerminationCause`, typed, on every path. Two causes are added:

```rust
pub enum TerminationCause {
    Completed { final_state: Option<Vec<u8>> },
    Failed { error: String },            // the trap / guest error text
    Stopped,
    Killed,
    PeerKilled { peer: String },         // a fate-link fired
    OwnerTerminated { owner: String },   // NEW: the owner died; ownership cascade (§4)
    Restarted { successor: String },     // NEW: replaced by restart_actor (§5); successor = new id
}
```

On the wire for actors there is one typed variant in the pacts, shared by both
callbacks:

```
variant termination-cause {
    completed(option<list<u8>>),
    failed(string),
    stopped,
    killed,
    peer-killed(string),
    owner-terminated(string),
    restarted(string),
    noproc,                // only in monitor replies (subject was not live)
}
```

Wisp gets it as `on-actor-down` / `on-child-exit` with
`e = (subject cause detail)`, e.g. `("…id…" "failed" "wasm trap: out of memory")`.
This is a printed form, so a session can act on it without decoding pack
bytes. (Wisp change; follow-up in the wisp repo.)

`Failed { error }` stays a string for now. A structured trap kind
(out-of-memory / unreachable / host error) belongs to the structured-errors
thread, not here.

### 3.4 `lifecycle` handler changes

- `monitor(subject)`: registers a down-watcher (`WatchActor`) instead of a
  chain subscription. One delivery per death, to the new export
  `handle-actor-down(subject: string, cause: termination-cause)` in
  `theater:simple/lifecycle-handlers`. It cannot flood and cannot form the
  §1.2.2 loop: a delivery into the watcher emits chain events on the watcher,
  and nobody's down-watcher reacts to those.
- `monitor-filtered(subject, pattern)`: unchanged in meaning (chain stream to
  `handle-actor-event`). The receive side becomes non-blocking: a drainer task
  reads the chain channel continuously, applies the pattern host-side, and
  pushes matches into a bounded local queue (e.g. 256). A separate task makes
  the wasm calls from the queue. On overflow the drainer drops the event and
  counts it; the next delivered event carries the count via a synthetic
  `monitor-dropped(n)` event-type. The subject is then never back-pressured by
  a watcher's wasm, which removes the deadlock. Completeness for this
  best-effort stream is traded away, as the `StateChain` doc already allows
  ("Best-effort — spawn a drainer task").
- `link(subject)`: also moves onto a down-watcher (no chain subscription). It
  keeps the `StopSelf` meaning, but filters to abnormal causes: not
  `Completed`, and not `Restarted` (a restart replaces the subject; it isn't a
  failure peers must share). That makes the code match the relationships doc
  §3.
- `link`/`monitor` of **self** → `err("cannot link/monitor self")`.
- `link` of a subject that is not live (`NoProc` / `AlreadyDown` abnormal) →
  the caller is stopped with `PeerKilled`, as in Erlang (linking to a dead
  process kills you unless you trap exits). This keeps the invariant "if I'm
  linked to X, X's death reaches me" race-free. Alternative for review:
  return `err("noproc")` instead.
- `unmonitor`/`unlink` → `UnwatchActor`.

`monitor-filtered` stays on the chain path because it wants arbitrary
non-terminal events, which only the chain has.

## 4. Ownership (0010 items 1 and 2)

### 4.1 Data

```rust
// on ActorProcess
parent: Option<TheaterId>,      // the spawner; None for a root (host-spawned)
children: HashSet<TheaterId>,   // live actors this one owns
```

Both live on the single-threaded command loop and change in exactly two
places: `register_actor` (insert into the parent's `children`, if the parent is
still live) and `finalize_actor` (remove from the parent's `children`). That
is the whole consistency story; the relationships doc's worry about a
"parallel index that drifts" applies to indexes kept by different owners, not
to one struct under one loop.

- `spawn_actor` already receives `parent_id`. If the parent is no longer live
  (or is `Stopping`) when the child registers, the spawn fails with
  `SpawnError::OwnerGone`. Otherwise the child would be born orphaned (the
  spawn-from-dying-parent race).
- `GetActors` returns the real `parent`: `list-actors` and the spawn stream
  agree.

### 4.2 The one rule: the owner's death stops its children

In `initiate_teardown(id, cause, force)`, after emitting the terminal and
firing down-watchers, for each child in `children`:

```rust
let _ = self.theater_tx.send(TheaterCommand::OwnerTerminated { actor_id: child, owner: id, force });
```

That command's arm calls `initiate_teardown(child, OwnerTerminated { owner }, force)`.
The cascade then ripples one hop per command, as the link cascade does: no
recursion on the loop, no central walk. Each level's terminal names the level
above it. `force` propagates (a force-killed owner force-kills its subtree; a
graceful stop is graceful all the way down).

Properties:

- It applies on **every** cause, `Completed` included. An owner finishing its
  job must not leave its workers holding sockets. This is unlike peer links,
  where `Completed` doesn't propagate. Ownership is not a peer relationship,
  and this matches OTP, where a supervisor's exit always takes its children
  with it.
- It is unconditional: no handler is involved, so a child without the
  lifecycle handler is still covered. That was the defect that got the old
  handler-driven cascade removed.
- **Order**: children are signalled concurrently, not in reverse start order.
  OTP stops children in reverse start order and waits for each. That ordering
  is policy, so it belongs to the supervisor library: a supervisor that needs
  ordered shutdown stops its children itself, in order, before it exits.
- **Ports are freed**: each child runs its normal bounded teardown, which
  closes its listeners.
- `ShutdownRuntime` stays a flat sweep. With ownership, a child may get both
  the sweep's `Stopped` and its owner's `OwnerTerminated`; the `Stopping`
  guard keeps whichever comes first.

Opting out (an owner that wants a child to outlive it) is not provided. The
use case is a daemon-style handoff, which can be expressed as "ask a long-lived
actor to spawn it". If a real need appears, `runtime.spawn` can take a
`detached` flag that sets `parent = None`. Left out until then.

### 4.3 Delivery to the parent's wasm

The parent's `runtime` handler instance creates the down-watcher channel at
handler creation. It passes the sender in every `SpawnActor` it issues (new
field `owner_watch: Option<DownSender>`; the runtime registers it before the
child is reachable). Its `setup` loop drains the channel:

- If the parent exports `theater:simple/runtime-handlers.handle-child-exit(id: string, cause: termination-cause)`,
  call it, through a bounded queue like §3.4's, so a slow parent never blocks
  the drain. The queue only ever holds one entry per child death, though.
- If it does not, log the death at `info` with the cause and drop it. The
  ownership cascade still applies either way.

`spawn-and-wait` already subscribes to the child's chain to find its terminal.
It switches to the owner watcher (the `Completed` → `final_state` mapping is
unchanged) and stops paying per-chain-event cost.

## 5. Restart

### 5.1 The spec

To start an actor again, the runtime needs what `spawn_actor` was given. Keep
it:

```rust
pub struct ActorSpec {
    pub name: String,
    pub manifest: Option<ManifestConfig>,
    pub wasm: Arc<[u8]>,       // the exact bytes that ran, not a re-fetch of manifest.package
    pub init_state: Value,     // the ORIGINAL init value (§6)
    pub call_init: bool,       // spawn (true) vs setup-only (false)
}
// on ActorProcess
spec: Arc<ActorSpec>,
```

- `wasm` is the bytes that were instantiated, not `manifest.package` resolved
  again, so a restart can't silently pick up different code (a moved file or
  an updated URL). An upgrade is a new spawn, not a restart.
- Memory: the bytes are already alive during spawn. Keeping an `Arc` per actor
  costs one module-size buffer per *distinct* spawn call site. When several
  actors come from the same resource-cache entry, the handler should hand over
  the cache's `Arc` instead of a copy. The cost lands on the
  spawn-per-connection pattern: worth measuring in S3 with
  `spawn-bench-baseline.md`. Fallback if it matters: store a content hash and
  keep bytes in the `ResourceCache` / content store.

### 5.2 Exited-child retention

A supervisor usually wants to restart a child **after** it died, by which time
`finalize_actor` has dropped it. So when an actor with a live owner finalizes,
its spec moves to the owner:

```rust
// on ActorProcess (the owner)
exited_children: HashMap<TheaterId, Arc<ActorSpec>>,   // old id -> spec
```

An entry leaves when the owner calls `restart(old)` (consumed), `forget(old)`,
or when the owner itself finalizes. A root's spec is dropped on finalize; a
host that wants to restart a root keeps its own spec (it spawned the root, so
it already has it). Unbounded growth would need an owner that never restarts
or forgets dead children while it keeps spawning new ones. The supervisor
library always does one or the other. As a backstop, cap `exited_children` per
owner (e.g. 1024, oldest evicted) and log the eviction.

### 5.3 `restart_actor`

```rust
TheaterCommand::RestartActor {
    actor_id: TheaterId,
    init_override: Option<Value>,          // NEW: replace the spec's init value (§6)
    response_tx: oneshot::Sender<Result<TheaterId, RestartError>>,  // CHANGED: returns the NEW id
}
pub enum RestartError { NotFound, OwnerGone, Spawn(SpawnError) }
```

Behavior, all on the command loop, never awaiting a teardown there:

1. **Live and `Running`**: record `pending_restarts[actor_id] = (spec, init_override, response_tx)`,
   then `initiate_teardown(actor_id, Restarted { successor }, false)`. The
   successor id is allocated now, so the terminal can name it: `spawn_actor`
   takes a pre-generated id. When `ActorShutdownComplete` for `actor_id`
   arrives, `finalize_actor` runs, then the pending restart spawns the spec
   with the pre-allocated id and the same parent. Starting only after teardown
   completes is what frees the old listener before the new one binds (the
   #225 lesson). If teardown doesn't ack within the stop deadline (8 s, as for
   `StopActor`), force-terminate and proceed.
2. **`Stopping`** (already dying): attach a pending restart to it; same as
   step 1 from the completion on. The terminal already emitted keeps its cause.
3. **Exited child** (`actor_id` in some owner's `exited_children`): take the
   spec and spawn now. To find the entry without a scan, keep one lookup map,
   `exited_index: HashMap<TheaterId, TheaterId>` (old id → owner), maintained
   alongside `exited_children`.
4. **Otherwise** → `NotFound`.

The successor:

- gets a **new id**. An id names one incarnation with one chain, and the old
  incarnation's watchers got their final `ActorDown`. This matches Erlang,
  where a restarted child is a new pid. Everyone who needs to find "the
  service" goes through a name (the registry), and the restarted actor
  re-registers in its `init`, as Erlang processes register at start.
- keeps the **same owner**. Ownership moves to the new id in the owner's
  `children`, and the owner's down-watcher is attached as at spawn.
- does **not** inherit explicit monitors or links. They were relationships
  with the old incarnation and have fired. A watcher that wants to follow the
  service re-monitors the successor, whose id is in the `Restarted` cause.
- runs `init` with the spec's init value (or the override), like a spawn.

### 5.4 Surface

`theater:simple/runtime` (mutate):

```
// Restart an actor from its spec. Works on a live actor (stop, then start) or on
// an exited child of the caller. Returns the new actor's id.
restart: func(id: actor-id, init-state: option<value>) -> result<actor-id, runtime-error>

// Drop a retained exited-child spec without restarting it. Idempotent.
forget: func(id: actor-id) -> result<_, runtime-error>
```

Authority: `runtime` ops are flat today (mutate may stop any actor), and
`restart` of a *live* actor follows that. Restarting an *exited* child
requires being its owner, because the spec is retained on the owner. The
check is "caller == owner in `exited_index`".

## 6. What state a restarted actor starts with

**The original init value, never the last state.**

- State is in-module: the runtime has no copy of "the last state". After a
  trap, wasm memory is exactly the state that failed (the inbox session's full
  heap). Restarting into it would reproduce the failure. This is also OTP's
  rationale: restart into a known-good state.
- State that must survive a crash lives outside the instance, in `store`, and
  `init` reloads it (inbox's mailboxes already do this). That is the
  application's decision, not the runtime's.
- `Completed { final_state }` in the terminal is unchanged and is *not* fed
  into a restart. It is a result for whoever waited (`spawn-and-wait`), not a
  resume point.

**Capabilities from the parent (0010 item 3).** A parent passes capabilities
such as a registry address to a child **in the init value**, e.g. a record
field `registry: string`. That needs no new mechanism, and because the spec
keeps the init value, a restart hands the child the same capabilities again
without the parent doing anything. Staleness is what the override is for: if
the registry itself was restarted (new id), replaying the old address would
give the child a dead registry. The supervisor library therefore holds a
"current capabilities" record. On restart it merges that record into the
spec's init value and passes the result as `init-state`. A parent that never
changes its capabilities passes `none`. 0010 item 3 then becomes a convention
plus this override, not a new runtime feature.

Considered and not proposed: an inherited, Unix-like environment (key/value
pairs copied from parent to child at spawn and re-read from the parent at
restart). It solves staleness automatically, but it adds a second channel for
starting state next to `init`, and an implicit one. Revisit if many unrelated
actors end up passing the same capability by hand.

## 7. Restart policy (userland supervisor library)

Policy is not in core. The decision logic is small, pure, and testable without
a runtime, so it is written once as a crate (`theater-supervision`, no I/O,
`no_std`-compatible so guests can compose it). It is used by the `supervisor`
actor, replacing `on_crash`'s ad-hoc limiter, and ported to Wisp in
`experiment/lib/supervisor.wisp` for sessions.

```rust
pub enum Strategy { OneForOne /* , OneForAll, RestForOne: later */ }
pub enum RestartType {
    Permanent,   // always restart
    Transient,   // restart unless the cause was Completed or Stopped (a normal end)
    Temporary,   // never restart
}
pub struct Intensity { pub max_restarts: u32, pub period_ms: u64 }   // OTP default 1 in 5 s; ours 3 in 5 s

pub struct ChildSpec { pub key: String, pub manifest: String, pub init: Value, pub restart: RestartType }
pub struct Supervisor { strategy, intensity, children: Vec<Child>, restarts: VecDeque<u64> }
pub struct Child { spec: ChildSpec, id: Option<String> }

pub enum Decision {
    Restart { key: String, old_id: String },
    Forget { key: String, old_id: String },          // temporary/transient-normal: drop the spec
    GiveUp { reason: String },                        // intensity exceeded: the supervisor must exit
    Ignore,                                           // not one of ours / already handled
}

impl Supervisor {
    pub fn on_child_exit(&mut self, id: &str, cause: &TerminationCause, now_ms: u64) -> Decision;
    pub fn on_restarted(&mut self, key: &str, new_id: String);
}
```

`on_child_exit`:

1. Find the child by current id; if none → `Ignore` (a stale notice).
2. Decide by restart type and cause. `Restarted` → `Ignore` (we did it).
   `OwnerTerminated` cannot reach us: we are the owner, and we are dying.
3. If restarting, drop entries older than `now - period` from `restarts`. If
   `restarts.len() >= max_restarts` → `GiveUp { "reached max restart intensity" }`.
   Otherwise push `now` → `Restart`.

The supervisor actor's `handle-child-exit` runs that decision, then:

- `Restart` → `runtime.restart(old_id, current_caps_merged_init)` → `on_restarted`.
- `Forget` → `runtime.forget(old_id)`.
- `GiveUp` → the supervisor terminates itself abnormally (returns `err` from
  the callback, or calls `self.shutdown` with a failure). The ownership rule
  stops all its children, and its own owner gets
  `handle-child-exit(supervisor, failed("reached max restart intensity"))`.
  That owner applies its own policy one level up. This is OTP escalation, and
  it needs no core support beyond §4 and §5.

`OneForAll` / `RestForOne` are later additions in the same crate (stop the
siblings, then restart them in start order via `restart`); no core change is
needed. A Wisp session can supervise too: define `on-child-exit`, call the
Wisp port, then `(runtime-restart id)`.

## 8. Staged plan with tests

Each stage is one PR, mergeable alone, with tests in `crates/theater-tests`
using the existing test actors (`link-test`, `monitor-test`, `panic-child`).

**S0. Stop the bleeding (lifecycle handler only; no ABI change).**
- Reject self-`link`/`monitor`.
- Non-blocking drain for chain-stream monitors (§3.4): drainer + bounded
  queue + drop count.
- Tests: *mutual monitors don't deadlock*. Two actors monitor each other; one
  takes 1000 calls; both stay responsive within a deadline, and the drop count
  is reported. Self-monitor returns err.

**S1. Down-watchers (core) + typed `monitor`.**
- `ActorDown`, `WatchActor`/`UnwatchActor`, `WatchReply`. Fired in
  `initiate_teardown`.
- `lifecycle.monitor` → down-watcher → `handle-actor-down(subject, cause)`.
  `link` on a down-watcher, abnormal-only. `termination-cause` variant in the
  pacts.
- `SubscribeToDowns` + `runtime.subscribe-to-downs`.
- Tests:
  - exactly one down per death, on each cause (stop, kill, trap via
    `panic-child`, self-complete);
  - monitor of an unknown id → immediate `noproc`;
  - monitor during teardown → `AlreadyDown(cause)`;
  - a watched actor making 10k calls produces 0 deliveries until it dies;
  - `link` doesn't fire on `Completed`;
  - the host deaths stream sees a root's trap with its error text.
- ABI: changes `monitor`'s delivery export, so it is fleet-visible. Rides the
  next lifecycle wave, like #206. The supervisor actor moves to
  `handle-actor-down` / `handle-child-exit`.

**S2. Ownership (0010 items 1 and 2).**
- `parent`/`children`, `SpawnError::OwnerGone`, `OwnerTerminated` command and
  cause, `GetActors` parent, owner down-watcher at spawn,
  `handle-child-exit` delivery, `spawn-and-wait` on the owner watcher.
- Tests:
  - `list-actors` shows the spawner as parent-id;
  - stopping a parent stops its children and grandchildren, each terminal
    naming the level above;
  - a parent that *completes* stops its children;
  - a child holding a TCP listener frees the port when its parent is stopped
    (the 09-14 outage);
  - spawn from a stopping parent → `OwnerGone`;
  - the parent's `handle-child-exit` sees `failed(...)` for a trapping child.
- Closes 0010 items 1 and 2.

**S3. Specs + restart.**
- `ActorSpec`, `exited_children` + `exited_index`, `pending_restarts`,
  `Restarted` cause, `RestartActor` returning the new id, `runtime.restart` /
  `runtime.forget`.
- Tests:
  - restart a live actor → new id, init ran with the original value, old
    terminal is `Restarted { successor }`;
  - restart a trapped child from `handle-child-exit` → new id, same parent;
  - restart of a stranger's exited child → permission error;
  - restart frees and rebinds the same TCP port;
  - restart with an init override is seen by the child's init;
  - an owner dying drops its exited specs;
  - spawn-bench before/after for spec retention memory.
- Closes the "not implemented" stub.

**S4. Policy library.**
- `theater-supervision` crate (pure; unit tests drive `on_child_exit` with
  synthetic clocks: permanent/transient/temporary × each cause; intensity
  window edges; give-up).
- Integration: a supervisor test actor with a `panic-child` that crashes on
  init N times. Expect restarts until intensity is exceeded, then the
  supervisor exits failed, its children are gone, and its owner's
  `handle-child-exit` sees the give-up.
- Port to Wisp in `experiment/lib`. Supervisor actor adoption is a separate
  ticket in its repo.

**S5 (outside this repo): `theater-repl` consumes deaths.**
- The daemon subscribes to downs and records the last cause per session; the
  death shows in `list`/`status`/`eval`'s error. Optionally a session-level
  `restart` command uses `RestartActor` (sessions are roots, so the daemon
  restarts while the actor is live, or re-spawns from its own spec after a
  death).
- Wisp: `on-actor-down`, `on-child-exit`, `(restart id [init])`,
  `(forget id)`.

## 9. Decisions for colin

claude approved the direction of this doc (2026-10-24). These three choices
are colin's.

### D1. The runtime holds lineage again (as data, plus one rule)

- **What #168 / #204 chose, and why.** Remove `parent_id`, the `children`
  index, `IsDescendant`/`GetDescendants`, and the runtime cascade, so that
  "the runtime holds no lineage at all". The cascade then lived in an opt-in
  handler with a 5 s poll: a parent's death could orphan children that didn't
  run that handler, and two uncoordinated mechanisms raced. The fix chosen
  was to make the runtime pure mechanism and leave supervision to a userland
  library.
- **Proposed.** Keep that layering for *policy* and *view-scope*. Bring back
  only lineage as data (`parent`, `children`) and one rule: an owner's
  termination stops its children. It runs through the same
  `initiate_teardown`, so nothing is opt-in and nothing races (§4). This is
  colin's goal for 0010 ("ancestry is real, so it can carry capabilities
  down"). claude supports it.
- **Alternative.** Stay lineage-free. Each parent must link its children
  itself. Today that means the child calls `link(parent)` (links are
  directed and self-service), so the guarantee depends on every child
  cooperating. A "link on behalf of" op would be needed to make it
  parent-driven. `list-actors` parent ids would come from the spawn stream or
  each supervisor's own records.
- **What breaks either way.** Proposed: any code that relies on children
  outliving their spawner. Nothing in-tree does; the supervisor actor already
  stops its own children on teardown, and the ownership rule makes that
  redundant but harmless. Spawning from a dying parent now fails with
  `OwnerGone`. Alternative: orphans continue (the 2026-09-14 socket-leak
  outage class), REPL sessions and any parent that isn't the supervisor
  actor get no cascade, and 0010 stays open.

### D2. `monitor` is terminal-only by default

- **What #206 chose, and why.** A bare `monitor` watches the whole chain,
  because "the chain is the actor's source of truth", with
  `monitor-filtered` (#205) to narrow it. #205 existed because unfiltered
  monitors on the mail spine re-triggered the chain-amplification wedge.
- **Proposed.** `monitor` = one typed down notice per death (§3.4), on a path
  that cannot back-pressure the subject. The whole-chain stream stays as an
  explicit opt-in: `monitor-filtered(subject, any)`, made non-blocking. claude
  supports it.
- **Alternative.** Keep whole-chain as the default and only add the
  non-blocking drain (S0). The deadlock goes away, but the default still
  amplifies (every subject event costs a wasm call in the watcher, and a REPL
  watcher's heap grows with the subject's activity). Every caller who wants
  "tell me when it dies" must know to filter. Wisp can't today.
- **What breaks either way.** Proposed: actors that rely on a bare `monitor`
  receiving non-terminal events must switch to `monitor-filtered(any)`. In
  tree that is `wasm_event_monitor_roundtrip_test` (#227); out of tree, probably the
  supervisor actor's flight recorder, if the post-#204 rebuild captures child
  chains with a bare `monitor`. I couldn't check: the local checkout predates
  the rebuild. The
  delivery export for terminal notices changes to `handle-actor-down`, a
  fleet-visible ABI change on the next lifecycle wave. Alternative: no ABI
  change; the flood and memory-growth hazard stays the default.

### D3. Restart gives the successor a new id

- **Proposed.** A new id per incarnation (§5.3): one id = one chain = one
  final down notice. Erlang does the same (a new pid). claude leans the same
  way.
- **Alternative.** Keep the id and add an incarnation counter. Holders keep
  working, but the chain must either continue across a crash (one hash-chain
  spanning two instances, with the dead one's tail) or fork under the same
  id. "Exactly one `Terminated` per id" stops being true, and every watcher
  must handle "down, then alive again" for the same id.
- **Cost of new ids, and how holders recover.** Anything holding the old id
  goes stale. Each kind of holder needs a recovery path:
  - *Registry bindings.* A lookup never returns a stopped actor, so the name
    reads `not-found` until rebound. The restarted actor re-registers in its
    `init`, using the registry address from its init value (§6). Convention:
    a service that registers a name does so in `init`, not once from outside.
  - *The parent / supervisor.* Told directly. `restart` returns the new id,
    and `on_restarted` updates the child record.
  - *Peers that monitored the old id.* They get `Restarted { successor }` as
    the cause, with the new id in it, and can re-monitor.
  - *The `theater-repl` daemon's session table (the ACTOR column).* It holds
    session name → actor. After a daemon-driven restart it updates the row
    (the session id/name stays; the ACTOR changes). Users who address
    sessions by name (`session-actor "claude"`) are unaffected. Ids that
    agents cached by hand go stale.
  - *An agent's notify target (`THEATER_NOTIFY`) and other ids copied into
    env or config.* These go stale and must be looked up by name again. That
    argues for notify targets to be names resolved through the registry; a
    follow-up for experiment, not theater.
  With the alternative (same id), none of these go stale, but D3's chain and
  "one terminal per id" problems apply.

### Smaller open questions

1. `link` to a non-live subject: stop the caller (Erlang; proposed) or return
   `err("noproc")`?
2. An owner's `Completed` stops its children (proposed), versus Erlang's
   "normal exit doesn't propagate". Ownership and peer links are treated
   differently on purpose.
3. A `detached` spawn: left out until there's a use.
4. **Spec retention memory.** The wasm bytes are an `Arc` shared with the
   resource cache where possible: one buffer per distinct module. The init
   value is a per-actor copy, so a child's spec costs the size of its init
   `Value`, and an exited child's spec stays on its owner until restart,
   forget, or owner death (capped, §5.2). For spawn-per-connection handlers
   (small init values, many actors) this is noise. For actors spawned with
   large init values it is the init value again. Measure in S3; the fallback
   is hash + cache for the bytes.

## 10. Relation to other docs and tickets

- `docs/actor-relationships-and-lifecycle.md`: keeps the vocabulary
  (`Terminated { cause }`), the link/monitor split, the emergent ripple, and
  "the runtime's only action is stop". It changes where death notices travel
  (a down-watcher, not the chain) and reinstates lineage as data with one
  rule. That doc's "as-landed" header should point here once S2 lands.
- Experiment 0010: items 1 and 2 are S2. Item 3 is §6 (init value + restart
  override). 0010 can be closed when S2 lands, or narrowed to "implement S2".
- `notes/` teardown socket leak (2026-09-14): fixed by S2's ownership rule.

## Problems noticed outside this ticket

- `StateChain`'s doc comment on `add_subscriber` says dispatch is via
  `try_send`; the code uses `send().await` (strict back-pressure). The struct
  doc is right; the method doc is stale.
- The relationships doc says `Completed` doesn't propagate a fate-link; the
  code's `link` uses `any_termination()`. S1 fixes this.
- The `RestartActor` doc comment says it is "only available to the actor's
  supervisor", but there is no supervisor concept in the runtime today and
  nothing sends the command.

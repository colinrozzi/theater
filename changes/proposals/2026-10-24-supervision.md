# Supervision: death notification and restart, on existing tools — design

Status: **proposal, for review** (do not implement yet). Revised after
colin's review of theater#235.
Author: uma (theater agent)
Ticket: experiment 0019; fits with experiment 0010.

How Theater behaves today is in
[docs/messaging-and-lifecycle-behavior.md](../../docs/messaging-and-lifecycle-behavior.md)
(theater#236). This doc cites it as "behavior §n" and doesn't repeat it.

## 0. Why

The coordinator's inbox session (a Wisp interpreter actor) trapped (out of its
fixed 2 GB heap) and nothing noticed; it was found only when a later eval
failed. Theater recorded the death: one `Terminated { Failed { <trap> } }`
on the session's chain (behavior §2.1). But a chain event goes only to that
chain's subscribers, and nobody had subscribed (behavior §2.2).

The direction from review:

- **No restart in the runtime.** Restart is a userland supervisor spawning
  again.
- **No runtime ownership.** A child that should die with its parent links to
  it, or a supervisor stops its children.
- **Death notices use the tools that already exist.**
- **Theater changes stay small**, because each one needs colin's review.

## 1. What exists today

Behavior §2 and §3 cover this, all verified at `99288971`. The parts this
design depends on:

- Every death path records exactly one terminal event carrying the cause.
  The `Stopping` guard in `initiate_teardown` makes it exactly one.
- Hosts can subscribe to every actor's chain with
  `TheaterRuntime::add_global_subscription` and keep only the events matching
  `any_termination()` (behavior §3.2).
- Actors can watch with `monitor-filtered(subject, any_termination())`.
  Matching happens in the host, so the watcher's wasm runs once, at the death
  (behavior §3.3).
- `SubscribeToActor` has no reply (behavior §4). So `link` and `monitor` of a
  dead or unknown id return `ok` and never fire.

### 1.1 Lineage

The runtime doesn't record who spawned whom, and stopping a parent doesn't
stop its children (behavior §2.3). Under this design that stays as it is.

### 1.2 Why a bare `monitor` floods, and the mutual-monitor deadlock

A bare `monitor` subscribes with `Pattern::any()`, the whole chain. Chain
delivery is strict (`.send().await`), and the lifecycle handler makes one
wasm call per matching event, serially. A REPL session emits chain events for
every eval, host call, and trigger firing: one `(+ 1 2)` in a watched session
produced two deliveries. A Wisp watcher buffers each one as a trigger firing
in a heap that never frees, so the watcher's memory grows with the
*subject's* activity.

**Repro of the deadlock** (private `theater-repl` daemon on another port):

1. Create sessions A and B.
2. In A: `(define on-actor-event (lambda (e) (car (cdr e))))`, then
   `(monitor "<B's actor id>")`.
3. In B: the same, monitoring A.
4. Run one `(+ 1 2)` in B.

From then on, both sessions stop answering (`eval` and `read` time out) at
~0% daemon CPU, until the daemon is taken down. Each delivery into A is a
wasm call that emits events on A's chain, and those go to B's handler, and
vice versa. Each delivery yields about two events back, until both 1024-slot
channels are full. Then each handler is blocked inside a wasm call, waiting
to send to the other side's full channel. A self-monitor is the same loop
with one actor.

A terminal-only pattern removes the loop: the watcher's wasm runs only when
the subject dies, so deliveries can't produce more deliveries.

## 2. Design

### 2.1 Death notification

**Who hears.** Whoever subscribed, and nobody else. There is no default
recipient in the runtime. Each kind of party has an existing way to
subscribe:

| Party | How it hears | Theater change |
|---|---|---|
| **A host** (the `theater-repl` daemon, theater-cli) | `add_global_subscription(tx)`, keeping only events that match `any_termination()`. It sees every actor spawned after the call, so subscribe at startup, before spawning anything. For actors that already exist, use `SubscribeToActor` per actor. | None. The daemon change is in the theater-repl repo. |
| **An actor** that wants to know when another dies | `monitor(subject)`, terminal-only after T3 below. Until then, `monitor-filtered(subject, any_termination())`. | T3 makes the plain verb safe. |
| **A parent or supervisor** | Monitors each child right after spawning it. If the child dies between the spawn and the monitor, the monitor call now reports that (T1). | T1 |

**What the notice says.** The terminal chain event: the
`Lifecycle(Terminated { cause })` payload, delivered as `data` to
`handle-actor-event` (or to the host's channel). `cause` is `Completed` /
`Failed { error }` (the trap text) / `Stopped` / `Killed` /
`PeerKilled { peer }`. No new message type is needed.

**Host-side caveat.** A global subscription is an ordinary chain subscriber,
and delivery is strict. A slow host reader slows every actor it subscribes
to, which for a global subscription is every actor (behavior §3.1). The host
must drain its channel promptly, e.g. with a drainer task that filters to
terminations and forwards into its own queue. The doc comments that claim
`try_send` are wrong (T6).

**Gaps outside Theater, as follow-ups:**

- *theater-repl daemon:* subscribe globally at startup and record each
  session's terminal cause, then show it in `list` / `status` / the failing
  `eval`'s error (e.g. "session claude-inbox failed: wasm trap: …"). This is
  what would have caught today's incident.
- *Wisp:* after T3, `(monitor id)` is terminal-only, so Wisp no longer needs
  to build a `Pattern` for the common case. It still can't read the cause:
  it's pack bytes in `data`. Wisp needs to decode the `ChainEventPayload`,
  or at least the termination cause, into `(subject cause detail)`.

### 2.2 Children that should die with their parent

The runtime does not do this. In experiment, the hub spawns every agent
session with `runtime.spawn` (`experiment/hub/src/lib.rs:362`). Runtime
ownership would stop every agent whenever the hub is replaced. Spawning is
not owning.

Where fate-sharing is wanted, there are two ways:

- **The child links to its parent.** The parent passes its own id in the
  child's init value, and the child calls `link(parent)` in `init`. `link`
  is directed and self-service, so this is the child's choice.
- **A supervisor stops its children** in its teardown, or before giving up
  (§2.3). It can do this in reverse start order if order matters, as OTP
  does.

0010 item 2 ("stopping a parent stops its children") is answered by these two
patterns rather than by a runtime rule. 0010 item 1 (real parent ids in
`list-actors`) is not part of this design. A host that wants lineage can keep
it from `SubscribeToSpawns`, which carries the parent id (behavior §2.3).

### 2.3 Restart and restart policy (userland)

Restart is a supervisor spawning again: a **new id**, starting from the
**original init value**, rate-limited. The `supervisor` actor already does
this (`on_crash` in the sentinel-era code: N restarts in M ms, then
blocked). The runtime's `RestartActor` is dead code and is removed (T5).

The policy belongs in a small library, so the supervisor actor and Wisp
sessions share one implementation. A pure crate, no I/O, usable from guests:

```rust
pub enum RestartType {
    Permanent,   // always restart
    Transient,   // restart unless the cause was Completed or Stopped
    Temporary,   // never restart
}
pub struct Intensity { pub max_restarts: u32, pub period_ms: u64 }   // e.g. 3 in 5 s

pub struct ChildSpec { pub key: String, pub manifest: String, pub init: Value, pub restart: RestartType }

pub enum Decision {
    Restart { key: String },   // the supervisor spawns spec.key again
    Drop { key: String },      // temporary, or transient that ended normally
    GiveUp { reason: String }, // intensity exceeded
    Ignore,                    // not one of ours (a stale notice)
}

impl Supervisor {   // one-for-one; one-for-all / rest-for-one later, same crate
    pub fn on_child_exit(&mut self, id: &str, cause: &TerminationCause, now_ms: u64) -> Decision;
    pub fn on_spawned(&mut self, key: &str, new_id: String);
}
```

The supervisor loop:

1. Spawn each child.
2. Monitor it (terminal-only), and link the child to the supervisor (§2.2)
   if the child should die with it.
3. On each terminal notice, run `on_child_exit`.
4. Act on the decision:
   - `Restart` → `runtime.spawn(manifest, init)`, then monitor the new id,
     then call `on_spawned`.
   - `GiveUp` → stop the remaining children and fail the supervisor itself.
     Whoever monitors the supervisor (its own supervisor) gets
     `Failed { "reached max restart intensity" }` and applies its own policy.
     That is OTP escalation, built from monitors.

The Wisp port lives in `experiment/lib/supervisor.wisp`; Wisp sessions
receive the notice through `on-actor-event`.

### 2.4 State on restart, and capabilities

A restarted child starts from the **original init value**, never its last
state:

- State is in-module. After a trap, wasm memory is the state that failed.
- State that must survive a crash goes to `store`, and `init` reloads it.

Capabilities such as a registry address are passed **in the init value**
(0010 item 3). The supervisor keeps each child's `ChildSpec`, init value
included, so a respawn hands the child the same capabilities without any
runtime support. If a capability went stale (e.g. the registry itself
restarted with a new id), the supervisor updates the spec's init value
before respawning. That is an ordinary `spawn` with a different argument.

A new id per restart means holders of the old id go stale. Each holder
recovers by name:

- A service re-registers itself with the registry in its `init`.
- The supervisor learns the new id from `spawn`.
- Peers that monitored the old id got its terminal notice, and look the
  service up again.

## 3. Theater changes (each small, each for colin's review)

**T1. `SubscribeToActor` returns a reply.**

```rust
TheaterCommand::SubscribeToActor {
    actor_id: TheaterId,
    event_tx: Sender<(TheaterId, ChainEvent)>,
    response_tx: Option<oneshot::Sender<SubscribeReply>>,   // NEW
}
pub enum SubscribeReply {
    Subscribed,
    NoSuchActor,
    AlreadyTerminated(TerminationCause),
}
```

In the runtime arm (rt:569):

- If the actor is `Stopping(cause)` → reply `AlreadyTerminated(cause)` and
  don't add the subscriber. Its terminal was already emitted.
- Else if its chain exists → add the subscriber and reply `Subscribed`.
- Else → `NoSuchActor`.

All three are decided on the command loop, which is also where the terminal
is emitted and the chain removed. So a subscriber either is in place before
the terminal, or is told why not. Nothing is missed silently.

The lifecycle handler's `add_subscription` awaits the reply (the host fns are
already async). For `link` / `monitor` / `monitor-filtered` it returns:

- `ok` for `Subscribed`;
- `err("no such actor")` for `NoSuchActor`;
- `err("already terminated: <cause>")` for `AlreadyTerminated`.

On an error it removes the local entry it just added. `Option` keeps a
fire-and-forget caller possible.

API note: adding a field breaks external struct-literal constructors of
`SubscribeToActor`. Nothing in this repo outside the lifecycle handler
constructs one; the theater-repl daemon should be checked.

Tests:
- monitor of an unknown id → `err`;
- monitor during teardown → `err` naming the cause;
- monitor of a live actor, then stop it → exactly one terminal delivered;
- link of an unknown id → `err`, and the caller keeps running.

**T2 (S0). Reject self-relations; non-blocking drain for chain-stream monitors.**

- `link` / `monitor` / `monitor-filtered` with subject == caller →
  `err("cannot link/monitor self")`.
- The handler's loop splits in two:
  - a drainer reads the chain channel continuously, matches in the host, and
    pushes matches into a bounded local queue (e.g. 256);
  - a second task makes the wasm calls from the queue.

  On overflow the drainer drops the event and counts it. The next delivery
  reports the count with a synthetic event-type `monitor-dropped:<n>`. A
  watcher's wasm then never back-pressures the subject, so a broad
  `monitor-filtered` can't deadlock. That stream becomes best-effort, which
  the chain doc already allows.

Tests:
- two actors with `monitor-filtered(any)` on each other, 1000 calls on one →
  both stay responsive within a deadline, and the drop count is reported;
- self-monitor → `err`.

**T3 (D2, kept). A plain `monitor` is terminal-only.**

- `monitor(subject)` uses `any_termination()` instead of `Pattern::any()`
  (`theater-handler-lifecycle/src/lib.rs`, l.339). The whole-chain stream
  stays available as `monitor-filtered(subject, any)`. The signature is
  unchanged; the semantics change. Update the pact comments.
- This breaks anything relying on a bare `monitor` seeing non-terminal
  events. In tree: `wasm_event_monitor_roundtrip_test` (#227), which switches
  to `monitor-filtered(any)`. Out of tree: check the supervisor actor's
  flight recorder.

Tests:
- a monitored actor making 10k calls → 0 deliveries until it dies, then
  exactly one;
- two mutually monitoring actors (plain `monitor`) stay responsive. This is
  the §1.2 repro as a regression test.

**T4. `link` does not fire on `Completed`.**

- The link filter becomes the match-any set of abnormal causes:
  `Terminated(Failed(_))`, `Terminated(Stopped)`, `Terminated(Killed)`,
  `Terminated(PeerKilled(_))`. Add it as `abnormal_termination()` in
  `theater/src/subscription.rs`, next to `any_termination()`.
- This matches the relationships doc §3.

Tests: the subject completes → the linked actor keeps running; the subject is
stopped or traps → the linked actor stops with `PeerKilled`.

**T5. Remove `RestartActor`.**

Remove the variant (`messages.rs:310`), its log arm (`messages.rs:489`), the
dispatch arm (rt:320), and `restart_actor` (rt:1157). Nothing sends it. The
build is the test.

**T6. Doc fixes.**

- `add_subscriber` (`chain/mod.rs:147`) and `add_global_subscription`
  (rt:274) say `try_send` and dropped events; the code waits. Say "strict:
  drain promptly, or a slow reader slows every actor it watches".
- Update `docs/messaging-and-lifecycle-behavior.md` §3.3 / §4 in the same PR
  as T1, T3 and T4.

**Order:**
- T5 and T6 are independent, trivial, and can go first.
- T1, then T3 + T4 (one lifecycle PR, one ABI-semantics note for the
  fleet), then T2.
- Follow-ups outside this repo: theater-repl daemon death display (§2.1),
  Wisp cause decoding, the supervision crate and its Wisp port (§2.3).

## 4. Decided in review (colin, 2026-10-24)

- No `restart_actor` in the runtime; restart is userland respawn (new id,
  original init value). `RestartActor` is removed (T5).
- No runtime ownership. Fate-sharing is a child's `link` or a supervisor's
  stop, because the hub spawns all agent sessions.
- Death notices use existing tools: `add_global_subscription` for hosts,
  terminal-only monitors for actors. No new down-watcher primitive.
- Keep: the `SubscribeToActor` reply (T1), S0 (T2), terminal-only `monitor`
  (T3), `link` not firing on `Completed` (T4).

Open, small:
1. `link` / `monitor` of a dead actor returns `err` (proposed). The Erlang
   alternatives are an immediate `noproc` notice for a monitor, and stopping
   the caller for a link. `err` is simpler and the caller can act on it.
2. T2's drop-count signal: a synthetic event-type, or a counter readable from
   the handler?

## Problems noticed outside this ticket

Listed in behavior §4 (theater#236): the router stalls on a full mailbox or a
slow channel accept, deferred request replies don't work, and request
failures look like empty replies.

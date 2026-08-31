# Actor Relationships & Lifecycle Subscriptions — Design

**Status:** IMPLEMENTED (PR #167) + runtime made pure mechanism (follow-on).
**Date:** 2026-08-29 (design) / 2026-08-31 (landed).
**Context:** follow-on to the runtime-owned actor tree (PR #164). Supersedes the
ad-hoc "supervision tree + handler-driven cascade" model.

> **As-landed architecture (read this first — it overrides the runtime-tree
> framing below).** The runtime holds **no lineage at all**: just a flat set of
> live actors. Every relationship (link / monitor) lives in the `lifecycle`
> handler, which subscribes to the subject's chain, matches events host-side, and
> acts per `Target` — `StopSelf` issues `PeerTerminated` (fate), `DeliverToWasm`
> calls the actor's `handle-lifecycle-event` (watch). The death cascade is
> **emergent**: each death emits its terminal chain event, linked peers' handlers
> match it and stop themselves, and their deaths ripple the same way — one hop per
> death, no central walk. **Supervision is the supervisor handler's job**: it
> tracks its own direct children, stops them on its teardown, and answers
> **view-scope from that direct-children set** (`scope: subtree` = the caller's
> direct children; deeper actors belong to child supervisors — the Erlang
> hierarchy). There is no `parent_id`, no `children` index, no `IsDescendant` /
> `GetDescendants`, and no runtime subscribers map. Sections below that describe a
> runtime-owned tree or an auto child→parent link are historical design context.

## 1. Motivation

Theater's supervision has grown as a single strict-ancestry tree that conflates
three genuinely different relationships into one `parent_id` edge:

1. **Ownership / lineage** — who spawned whom; restart authority; the basis for
   view-scoping.
2. **Fate-sharing** — "when X dies, Y must stop too" (the cascade).
3. **Watching** — "tell me about X's events, but I'm otherwise unaffected."

Conflating them caused concrete problems:

- The cascade lived in the **supervisor handler** (best-effort: opt-in per actor,
  async, 5s poll timeout) so a parent's death could **orphan** children that ran
  no supervisor handler. An invariant implemented in an opt-in handler isn't an
  invariant.
- The one "watch" mechanism we have (`subscribe-to-actor`) delivers **every**
  chain event of the subject into the subscriber's wasm — you pay a wasm call to
  pattern-match-and-discard even for events you don't care about.

Erlang/OTP is the precedent here, and it decomposes exactly along these lines:

- The **BEAM VM** provides **links** (symmetric fate-sharing between *any two*
  processes — an arbitrary graph, not a tree) and **monitors** (asymmetric,
  non-fatal death-watch). These are the unconditional primitives; link
  propagation happens even on a brutal `exit(Pid, kill)`.
- **OTP `supervisor`** is a *library* running in an ordinary process. It holds
  its own children list and applies **policy** (restart strategy, ordering,
  intensity limits) by `trap_exit`-ing the same signals.

Two takeaways we adopt:

- **The tree is a *pattern*, not a primitive.** The fundamental thing is a graph
  of directed relationships; supervision is one usage of it.
- **Mechanism in the core, policy in a handler**, wired to *one* event stream.
  (In Erlang the supervisor never polls — it's *told* via the same signals the
  VM propagates. Our 5s poll was the symptom of two uncoordinated mechanisms.)

## 2. The core primitive: a lifecycle subscription

Everything below is **one directed primitive**:

```
subscription {
    subscriber: TheaterId,   // who is attaching
    subject:    TheaterId,   // to whom
    filter:     EventFilter, // which of subject's chain events matter
    target:     Target,      // what happens on a matching event
}
```

- **Directed.** `A → B` means "A cares about B." Bidirectional is just two
  subscriptions (`A→B` and `B→A`) — no symmetric special-case to implement, and
  we get asymmetry for free (Erlang has to *recover* it with `trap_exit`).
- **`filter`** is a set/predicate over **raw chain-event types**. This subsumes
  coarse lifecycle states: "on death" is just `filter = { <terminal event> }`;
  "on error" is `{ <error event> }`; the current firehose is `filter = all`.
  Filtering happens **host-side**, so a non-matching event never crosses into
  wasm.
- **`target`** is where the matching event is *consumed* — the axis that
  distinguishes a link from a monitor:

  | `target`          | meaning                                              | consumed by |
  |-------------------|------------------------------------------------------|-------------|
  | `stop-self`       | the subscriber is stopped (**fate / link**)          | runtime core (no wasm) |
  | `deliver-to-wasm` | the event is delivered to the subscriber's handler (**monitor**) | actor space |

**A link and a monitor are the same subscription with a different `target`.** The
reason a link "lives in a different place" is that its target (`stop-self`) is a
**runtime-core action** — the notification never enters wasm, the actor is just
stopped. A monitor's target (`deliver-to-wasm`) hands the event to the actor to
react to.

### The two axes as a grid

|                     | `target: stop-self` (runtime) | `target: deliver-to-wasm` (actor) |
|---------------------|-------------------------------|------------------------------------|
| `filter: terminal`  | **link** (fate)               | death-monitor                      |
| `filter: subset`    | *(reserved; see §6)*          | filtered monitor                   |
| `filter: all`       | —                             | today's `subscribe-to-actor`       |

`subscribe-to-actor` is the single most expensive cell — `filter=all, target=wasm`
— and the model lets us express the whole grid instead of only that corner.

**Erlang parity, for free:** Erlang links propagate only *abnormal* exits. That's
just `filter = { abnormal-terminal }, target = stop-self` here — the policy lives
in the filter, not a special case.

## 3. What the runtime stores and does

Subscriptions are stored **on the subject** — one map per actor, on its
`ActorProcess`, keyed by subscriber:

```rust
struct ActorProcess {
    // ...
    subscribers: HashMap<TheaterId, Subscription>,  // who attaches to ME
}
```

This is a deliberate **single source of truth** — *not* a central table plus a
reverse index. A parallel `subscriber -> subjects` index would be pure
denormalized bookkeeping that every add/remove must touch in lockstep or it
drifts (the exact class of bug we're designing out). Instead:

- **Dispatch is O(1) and in-place:** when we handle B's event we already hold B's
  `ActorProcess`, so `B.subscribers` is right there.
- **Subject death is self-cleaning:** `deregister_actor(B)` drops B's process and
  its `subscribers` map with it — nothing else to remember to clean.
- **Subscriber death is lazy:** we don't keep a reverse index to eagerly prune a
  dead subscriber from every subject it watched. When we dispatch to a subscriber
  that is no longer a live actor, we skip and drop it in place. Correct in every
  ordering (dead subscriber → `stop-self` is a no-op; monitor delivery is
  skipped), and it only leaves a bounded set of dead entries that are pruned on
  contact. (The one thing this doesn't give cheaply is "list *my own* outgoing
  links" — an O(N) scan — but that's introspection, not a hot path.)

This is Erlang's shape: links live in each process's own control block, not a
central registry. And PR #164's `children` map is exactly the `target ==
stop-self` subset of `ActorProcess.subscribers` — "X's children" is "who
fate-attaches to X" — so the tree work folds in rather than sitting beside it.

On each chain event `e` emitted by subject `B`:

1. Take `B.subscribers` (in hand already).
2. For each, test `e` against its `filter` (host-side; cheap set membership).
   Drop any subscriber that is no longer live (lazy prune).
3. Dispatch matches by `target`:
   - `stop-self` → the runtime stops the subscriber. **No handler, no wasm.**
   - `deliver-to-wasm` → deliver `e` to the subscriber via the lifecycle handler
     export (below).

**The runtime action set is exactly one: `stop-self`.** Anything richer (restart,
custom reactions) is a `deliver-to-wasm` monitor where the *actor* decides — this
keeps the runtime dumb (Erlang's VM only *kills*; everything else is library) and
avoids growing a runtime-action framework. A second built-in action can be added
later if a real need appears, but the default is: fate is the only thing the
runtime enacts.

### Cascade is emergent

There is no central subtree walk. One local rule, applied at the single death
event:

> When an actor dies, the runtime enacts every `stop-self` subscription keyed on
> it — i.e. stops its fate-dependents.

Each such stop is itself a death → the rule fires again → the subtree tears down
as a **ripple**. This covers *all* death paths uniformly (graceful stop,
self-shutdown, crash) because it hooks the death **event**, not a specific call,
and there is only one mechanism so nothing races.

### The lifecycle-event vocabulary (decided)

Lifecycle transitions get a **fixed, typed vocabulary** — a new
`ChainEventPayload::Lifecycle` variant in `crates/theater/src/events/`, replacing
the ad-hoc `"shutdown"` / `"wasm"`-for-death `event_type` strings (and retiring
the dead `RuntimeEventData` / `TheaterRuntimeEventData` enums). It consolidates
the existing `ActorResult` (`Success` / `Error` / `ExternalStop`) into one place
that fires on **every** path.

```rust
pub enum ActorLifecycleEvent {
    Spawned,                             // setup + init done; actor is live
    Paused,
    Resumed,
    Terminated { cause: TerminationCause },
}

pub enum TerminationCause {
    Completed { final_state: Option<Vec<u8>> }, // clean self-driven exit  (← Success)
    Failed    { error: ActorError },            // guest error / trap      (← Error)
    Stopped,                                     // graceful external stop  (← ExternalStop)
    Killed,                                      // brutal force-kill
}
```

**Fate propagation default (resolves §7.2):** `Failed`, `Stopped`, and `Killed`
propagate a fate-link; **`Completed` does not** — a child finishing its job
cleanly must not tear down its fate-peers. That's the Erlang default ("normal
exit doesn't propagate"), expressed as a filter over the cause rather than a
special case.

## 4. Lineage, spawn, and view-scope

- **Spawn** records two things: the **ownership** fact (`parent_id`, kept — it's
  the spawn lineage and restart authority) *and* an implied **fate-link**
  `child → parent` (`filter = terminal, target = stop-self`). So supervision fate
  is auto-wired at spawn; no actor has to opt in for the tree to stay rooted.
- **Explicit `link`/`unlink`** additionally lets non-lineage peers share fate
  (e.g. a pipeline or mesh cohort that should all go down together — no parent
  among them).
- **View-scope** (`is-descendant`, the supervisor's "can I touch this actor")
  reads the **fate-link graph** the runtime already holds. "View = fate-reachable
  set," which generalizes "subtree" (see §6 open question on whether that's the
  scoping we want).

## 5. The new `lifecycle` handler (capability surface)

A new low-level handler is the *actor-facing* API; the runtime is the engine.

- **Ops (host functions):**
  - `link(subject, filter)` — write a `stop-self` subscription.
  - `monitor(subject, filter)` — write a `deliver-to-wasm` subscription.
  - `unlink(subject)` / `unmonitor(subject)`.
- **Export (for `deliver-to-wasm`):** `handle-lifecycle-event(subject, event)`.
- **Permission-gated** like every other capability (see §6).

Everything else recasts as a *consumer* of this substrate:

- `subscribe-to-actor` = `monitor(subject, filter = all)`.
- The `handle-actor-error` / `handle-actor-exit` / `handle-actor-external-stop`
  callback trio → filtered monitors on the terminal event → **the
  lifecycle-callback-consolidation** collapses into this automatically.
- The **supervisor handler** keeps only *policy*: it `monitor`s its children
  (`filter = death/error, deliver-to-wasm`) to drive restart strategy/ordering,
  and keeps a children list for that policy. **Fate is runtime links it no longer
  manages** — the handler's old shutdown-cascade + 5s poll is deleted.

## 6. Layering summary

| Layer | Owns |
|---|---|
| **Runtime core** | subscriptions on each `ActorProcess` (`subscribers`, single source of truth, lazy prune); host-side filtering; dispatch; the one action `stop-self`; the single death/chain-event stream. |
| **`lifecycle` handler** | actor-facing capability (`link`/`monitor` + filter), `handle-lifecycle-event` delivery. |
| **`supervisor` handler** | policy only — restart strategy/ordering, view-scope — as a consumer of the above. |

## 7. Open questions / decisions to make

1. ~~**Terminal event.**~~ **RESOLVED** — see the lifecycle-event vocabulary in
   §3: one typed `Terminated { cause }` event, fired on every death path.
   (Filter *representation* — how a subscription names a set of these types on
   the wire — is still to pin when the substrate is built.)
2. ~~**Normal vs abnormal death.**~~ **RESOLVED** — `Failed`/`Stopped`/`Killed`
   propagate fate; `Completed` does not (§3).
3. **Permissions.** Who may `link`/`monitor` whom — is it view-scoped like the
   supervisor ops (you can only attach within your view), unrestricted, or its
   own grant? Links create fate coupling, so this needs a real answer.
4. **View-scope semantics.** Keep "view = fate-reachable set," or keep an
   explicit ownership subtree distinct from arbitrary fate-links? (Peer links
   would widen a naive fate-reachable view.)
5. **Fleet-ABI impact.** New `lifecycle` interface + dropping the
   `handle-actor-*` callback trio is fleet-breaking; it rides the same wave as
   the other reshape work.
6. **`stop-self` shutdown type.** When a link fires, is the dependent stopped
   gracefully or force-killed, and can the subscription say which?

## 8. Relation to current code

- **PR #164** (runtime-owned tree: `parent_id` + strict-live-inverse `children`
  index + `IsDescendant`/`GetDescendants`) is the **first instance** of this
  model: `children[parent]` becomes the `target == stop-self` subset of
  `parent`'s `ActorProcess.subscribers`, and `is-descendant` is fate-reachability
  over it. When the subscription substrate lands, the central `children` map is
  absorbed into per-process `subscribers` rather than kept alongside it.
- **Next threads**, in dependency order:
  1. The single death/terminal chain event (shared with structured-errors /
     consolidation).
  2. The subscription substrate + host-side filtering + dispatch (`stop-self` /
     `deliver-to-wasm`) in the runtime core.
  3. The emergent cascade (the `stop-self` ripple on the death event) — replacing
     the reverted central-walk cascade.
  4. The `lifecycle` handler; recast `subscribe-to-actor` and the supervisor
     handler onto it; delete the handler-driven cascade + the `handle-actor-*`
     trio.

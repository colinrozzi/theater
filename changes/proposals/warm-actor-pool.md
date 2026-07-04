# Warm actor pool / connection dispatch — design

Status: **proposal, for review** (do not implement yet)
Author: theater-dev
Ticket: claude@ id=287 — "warm actor pool / connection-dispatch to replace
spawn-per-connection for stateless handlers"

This is the design rationale for a Theater primitive that dispatches an inbound
TCP connection to a **pool of pre-warmed, reusable handler actors**, instead of
`supervisor.spawn`-ing a fresh actor per connection. It is the root-cause fix
for the accept-loop wedge (`notes/inbox-accept-loop-wedge.md`) and for
cold-start latency.

The doc is deliberately opinionated about the *mechanism* (what the runtime does
today, what has to change) and deliberately **not** opinionated about the two
axes where Colin has said he has a preference: where the pool lives, and the
isolation/reuse model. Those are laid out as options with a recommendation, not
a decision.

---

## 1. What happens today (the mechanism we are replacing)

Two accept patterns exist in the tree; inbox uses the first.

**Built-in accept loop.** `tcp.listen(addr)` binds a listener and spawns a
background tokio task (the accept loop, `theater-handler-tcp/src/lib.rs:694`).
Per accepted connection it:

1. performs the **TLS handshake inline** (`lib.rs:716`, `acceptor.accept().await`),
2. inserts the connection into the shared map in `Pending` state,
3. calls `handle-connection` on the listening actor, in a **detached
   `tokio::spawn`** (`lib.rs:754`) — this is PR #67, added so a slow handler
   cannot wedge the accept loop.

**Acceptor actor (inbox).** Its `handle-connection` export, per connection:

1. `supervisor.spawn(handler_manifest, router_id)` — a fresh handler actor
   (`acceptor/src/lib.rs:312`),
2. `tcp.transfer(conn_id, handler_id)` (`:319`),
3. on transfer error, `supervisor.stop_child(handler_id)`.

**`tcp.transfer(conn_id, target_actor)`** (`lib.rs:1045`):

1. flips `entry.owner = target_actor` and `state = Active` in the shared map,
2. `GetActorHandle { actor_id: target_actor }` from the runtime,
3. **`target_handle.call_function("…/handle-connection-transfer", conn_id).await`**
   (`lib.rs:1104`) — awaits the target to *finish* handling the connection.

**Handler actor (inbox api-handler).** `init` loads secrets from the store once;
`handle-connection-transfer` does receive → route → RPC to mailbox → send → close,
then **`shutdown(None)`** (`api-handler/src/lib.rs:312`) — the actor is thrown
away after one connection. Its entire state is static config
(`HandlerState { router_id, dkim_private_key_pem, bearer_token }`); smtp-handler
is the same shape (`SmtpHandlerState { router_id }`, full session, then shutdown).

### Why this serializes (the wedge)

Each actor is a single wasm instance driven by **one mpsc operation channel**,
processed serially (`actor/handle.rs:77`, `ActorOperation::CallFunctionPack`).
So even though PR #67 detaches each `handle-connection` call into its own
tokio task, all those calls reconverge on the **acceptor's single operation
channel** and run one at a time. And each one does `spawn` (~10ms, per
`docs/spawn-bench-baseline.md`) then `transfer`, which **awaits the child's
entire request lifecycle** — receive, an RPC round-trip to the mailbox, send,
close. The acceptor is therefore pinned for the full duration of every
connection. Throughput ceiling ≈ 1 / connection-latency, single-file.

This is why PR #67 did not stop the recurrence
(`notes/inbox-accept-loop-wedge.md` §"Recurrence #5"): the detach moved the
block off the *tcp accept loop* but not off the *acceptor actor*. Under :443
polling churn the acceptor falls behind, `Pending` connections pile up, Recv-Q
climbs, HTTPS wedges.

Two independent serialization points, then:

- **S1 — inline TLS handshake** in the tcp accept loop (`lib.rs:716`).
- **S2 — the acceptor actor blocked per-connection**, because `transfer` awaits
  full handling on a serial op channel.

A warm pool as usually imagined (reuse actors, skip spawn) addresses cold-start
and spawn cost, but **does not by itself fix S2** — if dispatch still awaits the
handler, the dispatcher is still pinned. So the design's load-bearing change is
*non-blocking dispatch*, and the pool rides on top of it.

---

## 2. Feasibility verdict: transfer is the foundation, not a rewrite

The strong lead in the ticket is correct. `handle-connection-transfer` and the
transfer plumbing are the right foundation:

- `transfer` targets **any live actor by id** — it looks the target up via
  `GetActorHandle` and calls its export. Nothing anywhere requires the target to
  have been freshly spawned. A long-lived warm actor with a valid handle works
  **today, unchanged**.
- The shared-state design (`SharedTcpState` behind an `Arc`, all handler
  instances share the same connections map, `lib.rs:367`) is exactly what lets
  one actor's connection become another actor's connection. That is already how
  transfer works; a pool member is just a transfer target that outlives the
  connection.
- Connection ownership, activation, and the `Pending → Active` handoff are
  already atomic in the map under the connections mutex.

So there is **no rewrite of the connection model**. What is missing is small and
additive, in two buckets:

**Gap A — nobody keeps warm actors alive or picks among them.** Today the
acceptor spawns a fresh target every time. A pool needs: spawn N once, hold their
ids, choose one per connection, and recycle them.

**Gap B — dispatch blocks the dispatcher.** `transfer` awaits
`handle-connection-transfer`. For a pool to run N connections concurrently across
N members, the dispatch step must return as soon as the connection is *handed
off*, not when it is *handled*.

Gap B is the important one and is a ~10-line change in the runtime (spawn the
`handle-connection-transfer` call detached, return after the ownership flip).
Gap A is where the real design choices live (§4, §5).

---

## 3. The core primitive

A pool is:

- a **manifest** (the handler to warm) + **init config** (e.g. router_id),
- a **target size N** of pre-initialized, alive members,
- a **dispatch operation**: given an accepted `conn_id`, hand it to a member
  **without blocking** on the member finishing,
- a **readiness / backpressure** rule for when all members are busy,
- a **recycle** rule for retiring and replacing members,
- **crash isolation**: a member dying fails only its in-flight connection.

Non-blocking dispatch is the invariant that makes it work: *dispatch enqueues,
the member handles, the dispatcher moves on.* Members run concurrently because
each has its own operation channel and its own tokio-driven wasm execution.

---

## 4. Axis 1 — where does the pool live? (Colin's call)

Three placements, cheapest-first. All three depend on the Gap-B non-blocking
change; they differ in how much of the pool logic is runtime vs wasm.

### Option A1 — consumer-side pool in wasm (thinnest runtime change)

The acceptor spawns N handlers at `init`, holds their ids, and round-robins
`transfer` across them; handlers drop their `shutdown()` and simply return to
await the next transfer. Runtime change is **only** Gap B (make transfer, or a
new `transfer-async`, non-blocking).

- Pros: minimal runtime surface; pool policy is app code, easy to iterate; proves
  reuse + non-blocking dispatch end-to-end fast.
- Cons: dispatch is **blind round-robin** — the acceptor has no idea which member
  is idle, so a slow connection on member k queues new connections behind it in
  k's mpsc buffer even while other members sit idle. Backpressure is per-member
  buffer overflow, not a real ready-queue. Recycle logic is hand-rolled in every
  consumer. No central visibility.

### Option A2 — host-side pool primitive (most robust)

A new host interface owns pool lifecycle and dispatch. Sketch:

```
interface actor-pool {
    // create N pre-initialized members from a manifest + init config
    create: func(manifest: string, init-state: option<value>, opts: pool-opts)
        -> result<string, string>            // returns pool-id
    // hand an accepted connection to a ready member; returns immediately
    dispatch: func(pool-id: string, connection-id: string)
        -> result<_, string>
    stats: func(pool-id: string) -> result<pool-stats, string>
}
record pool-opts {
    size: u32,
    max-queue: u32,            // bounded backlog when all busy (§6)
    recycle-after: option<u32> // retire a member after N connections (§5)
}
```

`dispatch` picks a **ready** member (a real idle-set / ready-queue), flips
ownership, spawns the `handle-connection-transfer` call detached, and returns.
When that call resolves the member goes back to the idle set and the next queued
connection is pulled. If no member is idle, the connection queues up to
`max-queue`; past that, dispatch returns an error the acceptor turns into a
close / 503 (natural backpressure — strictly better than today's unbounded
spawn). Recycle and crash-replace live here, so every consumer gets them free.

- Pros: real readiness-based dispatch; central bounded backpressure; recycle and
  crash-replacement built in and uniform; one place to instrument (`stats`);
  reusable by every acceptor, not just inbox.
- Cons: most code. Spans tcp + supervisor (it must `spawn`/`stop` members), so
  it either extends the supervisor handler or is a new handler that depends on
  both. New public interface to commit to.

### Option A3 — hybrid: host picks, wasm owns lifecycle

Keep pool spawn/recycle in the acceptor wasm (as A1), but add a thin host helper
`transfer-to-ready(conn_id, [member_ids]) -> chosen_id` that picks the member
with the shortest operation queue (or an explicit idle flag) and does the
non-blocking handoff. Gets readiness-aware dispatch without committing to a full
pool-lifecycle interface.

- Pros: solves A1's blind-round-robin flaw with a small primitive; lifecycle
  stays flexible in app code.
- Cons: readiness ("shortest op queue") is a proxy the runtime can see but isn't
  as clean as an explicit idle-set; recycle still hand-rolled per consumer.

**Recommendation:** **A2** as the real target — it is the only option that
delivers readiness-based dispatch, central bounded backpressure, and uniform
recycle/crash-replace, and it is a primitive every future acceptor reuses. But
land the **Gap-B non-blocking transfer first as its own change** and validate it
with an **A1 prototype** (inbox acceptor round-robin over a fixed pool). That
prototype alone should take Recv-Q off :443 under the wedging load test, proving
the mechanism before we commit the larger A2 interface. If A1 already meets
inbox's needs, A2 can be scoped down or deferred.

---

## 5. Axis 2 — isolation / reuse model (Colin's tradeoff)

The handlers are provably stateless (state is static config, real data lives in
the mailbox/router/store over RPC), so reuse is *correct*. The question is how
much isolation we keep as defense-in-depth against a leak or a half-parsed
request bleeding into the next connection.

- **B1 — fresh per connection (status quo).** Max isolation, pays
  spawn+init+possible-compile every time. This is what we are moving off.
- **B2 — warm reuse + recycle (recommended middle).** A member serves many
  connections, then is retired and replaced after `recycle-after: N` connections,
  **and always immediately on any error or crash**. State-bleed exposure is
  bounded to at most one connection window and eliminated on the first sign of
  trouble; per-connection cost is ~0 in the common path. N is a dial: small N
  trades a little churn for tighter isolation, large N trades isolation for less
  churn. For stateless handlers even N large is safe; recycle-on-error is the
  part that actually matters.
- **B3 — warm reuse + explicit reset export.** Add a `reset` export the pool
  calls between connections to clear per-connection state. For genuinely
  stateless handlers this is a no-op, so B3 collapses into B2 with the reset as a
  cheap assertion. Worth it only if we expect handlers that *do* carry
  per-connection scratch state and want them reused rather than recycled.

**Recommendation:** **B2** — recycle-on-error unconditionally, plus a
`recycle-after: N` default (start conservative, e.g. N in the low hundreds) as
defense-in-depth. It gives ~all the performance of pure reuse while keeping the
blast radius of a bad connection to one member. Offer B3's `reset` hook as
optional for future stateful reuse, not required for inbox.

These two axes are independent: any A × any B is coherent. My combined pick is
**A2 + B2**, prototyped as **A1 + B2**.

---

## 6. Backpressure, crash isolation, recycle — the details

**Backpressure.** The bounded pool *is* the backpressure. In A2, `dispatch`
maintains an idle-set and a bounded FIFO of waiting connections (`max-queue`).
All members busy → queue; queue full → `dispatch` errors and the acceptor closes
the connection (HTTP 503 / SMTP 421). This is strictly safer than today's
unbounded per-connection spawn, which has no ceiling and is itself a driver of
the wedge. In A1 the only backpressure is each member's mpsc buffer filling
(then `transfer` would block the acceptor again) — a real reason to prefer A2.

**Crash isolation.** A member crashing (wasm trap, or its
`handle-connection-transfer` returning `Err`) must (a) fail only the in-flight
connection — close its `conn_id`, remove it from the shared map — and (b) remove
the dead member from the pool and spawn a replacement to restore size N. The
listener and other members are untouched. This is a superset of the current
error path (`acceptor/src/lib.rs:322` stops a child on transfer failure); in A2
it is centralized and also covers mid-handling crashes, not just handoff
failures. Note the tcp handler already cleans up a connection when
`handle-connection` errors (`lib.rs:768`); the pool needs the analogous cleanup
for `handle-connection-transfer`.

**Recycle.** On `recycle-after: N` reached (after the current connection
completes) or on any error: `supervisor.stop` the member, `supervisor.spawn` a
replacement from the same manifest+config (init runs once, re-loads secrets),
add it to the idle-set. Because init is amortized across N connections, the
~16s cold-fill / cranelift-compile outlier is paid once per member per recycle
cycle, off the connection hot path, instead of once per connection.

**Warm-up.** `create` spawns and inits all N members before the listener starts
dispatching (or dispatches degrade gracefully until the pool fills). First
traffic hits an already-warm pool → no per-connection compile.

---

## 7. Composition with the accept-loop patch

The ticket asks that dispatch-to-pool and the non-blocking accept-loop compose
cleanly. They do, and they are complementary — each removes one of the two
serialization points from §1:

- **S2 (acceptor pinned per connection)** is removed by the pool's **non-blocking
  dispatch** (Gap B). This is the change that actually lifts the throughput
  ceiling; the pool is what makes non-blocking dispatch *safe* (a bounded set of
  ready targets rather than unbounded spawn).
- **S1 (inline TLS handshake)** is *not* touched by the pool and should be fixed
  alongside: move `acceptor.accept()` (`lib.rs:716`) off the accept loop into the
  per-connection detached task, so a slow/stalled TLS handshake (a slow-loris
  client) can't hold up `listener.accept()`. Small, separable, worth shipping
  with or before the pool.

With both: the tcp accept loop only `accept()`s and detaches (S1 fixed); the
detached task hands off to a ready pool member without blocking (S2 fixed); N
members handle N connections concurrently; Recv-Q stays at 0 under polling load.

There is no ordering conflict — dispatch-to-pool *replaces* the `spawn` inside
the handoff; the non-blocking accept is still needed and sits upstream of it.

---

## 8. Rough size estimate

Staged, smallest-viable-first:

1. **Non-blocking transfer (Gap B)** — spawn the `handle-connection-transfer`
   call, return after the ownership flip; keep semantics via a new
   `transfer-async` (or a flag) to avoid changing existing `transfer` callers.
   Plus move the TLS handshake off the accept loop (S1). **~0.5–1 day**, mostly
   test.
2. **A1 prototype** — inbox acceptor spawns a fixed pool at init, round-robins,
   handlers drop `shutdown()`; a load test that wedges the current build and no
   longer does. Proves reuse + non-blocking dispatch + Recv-Q-stays-0.
   **~1–2 days** (spans a small inbox change, coordinate with inbox-dev).
3. **A2 host primitive** — the `actor-pool` interface, ready-set, bounded queue,
   recycle-after-N, crash-replace, `stats`, supervisor integration for
   spawn/stop, wasm-side bindings, and the load harness as an owned regression
   test. **~1 week+** including tests.

If step 2 already satisfies inbox in prod, step 3 can be descoped or deferred —
the design intentionally lets us stop early. My honest read: steps 1+2 are the
high-value core and are a few days; step 3 is the durable primitive and is the
week-scale piece.

---

## 9. Open questions for review

1. **Placement:** A1, A2, or A3 as the committed target? (I recommend A2, staged
   via A1.)
2. **Isolation:** B2 alone, or B2 + optional B3 `reset` hook? Default
   `recycle-after: N` — what N, or unbounded-with-recycle-on-error-only?
3. **`transfer` semantics:** make the existing `transfer` non-blocking, or add a
   separate `transfer-async` and leave `transfer` as-is for actors that rely on
   the await? (I lean new variant to avoid a silent behavior change.)
4. **Where A2 lives:** extend the supervisor handler, or a new
   `theater-handler-pool` crate depending on tcp + supervisor?
5. **Queue-full policy:** close/503 immediately, or a short bounded wait before
   rejecting?

# Messaging and lifecycle: what actually happens

A reference for how Theater **behaves today**, read from the source. Use it
before designing anything that depends on delivery, ordering, or death
notices, so the behavior doesn't have to be found again from the code.

- Every statement cites a file and line. All were checked at commit
  `99288971` (2026-10-24). If the code has moved since, re-check the
  statement before relying on it, and update this page.
- This page describes behavior, not intent. Where the code and another doc
  disagree, it says so ("Known gaps").
- Paths below are under `crates/`. "ms" means
  `theater-handler-message-server/src/lib.rs`, and "rt" means
  `theater/src/theater_runtime.rs`.

For the design and vocabulary of links and monitors, see
[actor-relationships-and-lifecycle.md](actor-relationships-and-lifecycle.md).

## 1. Message delivery

### 1.1 The router

All messages, channel opens, and channel messages go through **one router
task**, `MessageRouter::router_task`. Its command queue holds 10000 (ms:177).
It handles one command at a time.

Each actor gets a **mailbox of 100** messages when it calls `register()`
(ms:863). Before that, there is no mailbox, and sends to it fail (§1.2). A
per-actor task takes messages from the mailbox in order and calls the
actor's export. That call then goes through the actor's operation queue (also
100, rt:825), and the actor runs one operation at a time.

The router delivers with `mailbox.send(msg).await` (ms:281). So **when one
actor's mailbox is full, the router waits, and no other actor gets messages
until that mailbox has room.** This is backpressure, not loss. It is a known,
deliberately deferred issue.

### 1.2 `send` (one-way)

- The sender gets `Ok` once the message is **in the target's mailbox**
  (ms:277-282). It does not mean the handler ran or succeeded.
- The sender gets `Err("Actor not found")` only when the target has no
  mailbox: it never registered, or it stopped (ms:285).
- If the target's handler fails, the receiver logs it (ms:888). The sender is
  not told.
- Order: messages from one sender to one target arrive in order (the router is
  sequential and the mailbox is first-in, first-out). There is no ordering
  across different sender/target pairs.
- Nothing is persisted or retried. Messages in a mailbox when its actor stops
  are lost.

### 1.3 `request` (the caller waits for a reply)

- The caller gets the bytes that `handle-request` returned.
- **A failure looks like an empty reply.** If the handler returns an error or
  a reply that can't be parsed, or the call fails (a trap), the caller gets
  `vec![]` (ms:572, ms:577).
- **Deferred replies don't work.** `respond-to-request` and `cancel-request`
  (ms:1016, ms:1044) look the id up in `outstanding_requests`, but nothing
  inserts into that map: `process_actor_message` takes it as
  `_outstanding_requests` and never uses it (ms:540).
- There is no timeout of its own. The only bound is the 3000 s operation
  timeout on the export call (`theater/src/actor/types.rs:16`).

### 1.4 Channels

- **Open:** the router sends the open to the target and then **waits for the
  target's `handle-channel-open` answer inside the router task** (ms:325). For
  that time, message delivery stops for everyone (§1.1).
- **Message:** `Ok` means "in the other side's mailbox" (ms:394), as for
  `send`. Order is first-in, first-out. A failed handler is logged only.
- **Close:** best effort; the notice to the other side ignores send errors
  (ms:416-443).
- Nothing is persisted or retried.

## 2. Death

### 2.1 How actors die

`initiate_teardown` in rt is the single stop decision. It records one
`Lifecycle(Terminated { cause })` event on the actor's chain. A second stop is
a no-op. The causes are `Completed { final_state }`, `Failed { error }`,
`Stopped`, `Killed`, and `PeerKilled { peer }`.

- **A trap kills the actor.** A failed call sends `TheaterCommand::ActorError`
  (`theater/src/actor/runtime.rs:923`). `handle_actor_error` (rt:1032) then
  force-stops the actor with `Failed { <trap text> }`. Its graceful shutdown
  handlers do not run.
- **An export that returns a normal error value** (a WIT `err`, not a trap) is
  an ordinary return. The actor keeps running.

### 2.2 Who hears about it

Nobody, unless they subscribed. The terminal event goes only to the chain's
subscribers (§3). There is no default notice to a parent or to the host.

### 2.3 Lineage

The runtime does not record who spawned whom. `GetActors` always reports the
parent as `None` (rt:521-528). `SubscribeToSpawns` (`theater/src/messages.rs:379`)
does carry the parent id, so a host that wants lineage can keep it from that
stream. **Stopping a parent does not stop its children.**

### 2.4 Restart

There is none. `RestartActor` (`theater/src/messages.rs:310`, rt:320) reaches
`restart_actor` (rt:1157), which always returns "Actor restart not
implemented". Nothing sends the command. The current direction is to remove it.
Restarting is a supervisor spawning again: a new id, starting from its
original init value.

## 3. Watching other actors

### 3.1 The chain

Every actor has an event chain: export calls, host calls, lifecycle events.
**The runtime does not keep the events.** Each one is hashed, sent to
subscribers, and dropped; only the head hash is kept
(`theater/src/chain/mod.rs:6-10`). Anything that needs a record must
subscribe and store it.

**Delivery to subscribers is strict.** Each event is sent with
`.send().await` to each subscriber (`theater/src/chain/mod.rs:125`), so **a
slow subscriber slows the actor it watches**, and through it anything waiting
on that actor.

### 3.2 Host-side tools

| Tool | Gives | Notes |
|---|---|---|
| `TheaterRuntime::add_global_subscription(tx)` (rt:280) | Every chain event of **every actor spawned after the call** | Added as an ordinary chain subscriber (rt:851), so it is strict (§3.1): drain it quickly. theater-cli uses it (`theater-cli/src/commands/spawn.rs:179`). |
| `TheaterCommand::SubscribeToActor { actor_id, event_tx }` (rt:569) | One actor's chain, from now on | **No reply.** If the actor isn't running it only logs (rt:575), so the caller can't tell. |
| `TheaterCommand::SubscribeToSpawns` (`messages.rs:379`) | One row per spawn, with the parent id | |

To learn when any actor dies, a host subscribes globally and keeps only events
that match `theater::subscription::any_termination()`
(`theater/src/subscription.rs:40`).

### 3.3 Actor-side tools (the `lifecycle` handler)

All three subscribe the caller's handler to the subject's chain and match each
event **in the host** (`theater-handler-lifecycle/src/lib.rs`).

| Call | Pattern | On a match |
|---|---|---|
| `link(subject)` (l.306) | `any_termination()` (l.317) | The caller is stopped with `PeerKilled`. Directed: `A link B` stops A when B dies, not the reverse |
| `monitor(subject)` (l.328) | `Pattern::any()`: **the whole chain** (l.339) | `handle-actor-event(subject, event-type, data)` for **every event** |
| `monitor-filtered(subject, pattern)` (l.350) | The caller's pattern | `handle-actor-event` for matching events only |

**To be told when an actor dies, use `monitor-filtered(subject,
any_termination())`.** Matching happens in the host, so the watcher's wasm runs
once, at the death. The cause is in `data`, encoded as pack bytes.

A bare `monitor` is hazardous:

- **It floods.** Every event of the subject becomes a wasm call in the watcher.
  A watcher's memory can grow with the *subject's* activity.
- **Two actors that monitor each other deadlock** on the first call. Each
  delivery emits events on the watcher, which go back to the other side, until
  both channels are full and each handler waits on the other. A self-monitor
  does the same with one actor. (Reproduced on a private `theater-repl`
  daemon; see theater#235 §1.2.)

## 4. Known gaps

- `SubscribeToActor` has no reply (rt:569-578), so `link` and `monitor` of a
  dead or unknown id return `ok` and never fire, and an actor that dies while
  the subscription is being set up is missed. Planned fix: reply with
  subscribed / no such actor / already terminated with the cause.
- `link` fires on `Completed`, but the relationships doc says normal
  completion should not propagate.
- The doc comment on `add_global_subscription` (rt:274-279) and on
  `add_subscriber` (`theater/src/chain/mod.rs:147`) say delivery uses
  `try_send` and drops events. The code waits (`.send().await`).
- The `RestartActor` doc says it is "only available to the actor's
  supervisor"; there is no supervisor concept in the runtime.
- A full mailbox or a slow channel accept stalls the router for everyone
  (§1.1, §1.4).
- Deferred request replies don't work, and request failures look like empty
  replies (§1.3).

## 5. Keeping this page true

When you change any behavior above, update this page in the same PR. When you
learn something about delivery or lifecycle by reading the source, add it here
with a file and line, and the commit you checked.

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

There is none in the runtime. The unused `RestartActor` command, which only
ever returned "Actor restart not implemented", was removed (ticket 0053 T5,
2026-11-03). Restarting is a supervisor spawning again: a new id, starting
from its original init value.

## 3. Watching other actors

### 3.1 The chain

Every actor has an event chain: export calls, host calls, lifecycle events.
**The runtime does not keep the events.** Each one is hashed, sent to
subscribers, and dropped; only the head hash is kept
(`theater/src/chain/mod.rs:6-10`). Anything that needs a record must
subscribe and store it.

**Host calls are recorded generically.** A handler doesn't record its own
events. After every host function returns, `RecordingInterceptor` adds an
event of type `<interface>/<function>` (for example
`theater:simple/filesystem/rename`) holding the input and output
(`theater/src/interceptor.rs:55-64`). On replay,
`ReplayRecordingInterceptor::before_import` returns the next recorded output
for that function, and the host function isn't run
(`theater/src/interceptor.rs:134-143`). So a replayed filesystem write does
not touch the disk. (Checked at `d6148f98`, 2026-10-25.)

**Delivery to subscribers is strict.** Each event is sent with
`.send().await` to each subscriber (`theater/src/chain/mod.rs:125`), so **a
slow subscriber slows the actor it watches**, and through it anything waiting
on that actor.

### 3.2 Host-side tools

| Tool | Gives | Notes |
|---|---|---|
| `TheaterRuntime::add_global_subscription(tx)` (rt:280) | Every chain event of **every actor spawned after the call** | Added as an ordinary chain subscriber (rt:851), so it is strict (§3.1): drain it quickly. theater-cli uses it (`theater-cli/src/commands/spawn.rs:179`). |
| `TheaterCommand::SubscribeToActor { actor_id, event_tx, response_tx }` (rt:569) | One actor's chain, from now on | Optional reply (`SubscribeReply`): `Subscribed`, `NoSuchActor`, or `AlreadyTerminated(cause)` if the actor is already `Stopping` (its terminal was emitted; the subscriber isn't added). Decided on the command loop, where the terminal is emitted and the chain removed, so a subscriber either sees the terminal or is told why not. (Ticket 0053 T1, 2026-11-03; tests in `theater-tests/tests/supervisor_actor_test.rs`.) |
| `TheaterCommand::SubscribeToSpawns` (`messages.rs:379`) | One row per spawn, with the parent id | |

To learn when any actor dies, a host subscribes globally and keeps only events
that match `theater::subscription::any_termination()`
(`theater/src/subscription.rs:40`).

### 3.3 Actor-side tools (the `lifecycle` handler)

All three subscribe the caller's handler to the subject's chain and match each
event **in the host** (`theater-handler-lifecycle/src/lib.rs`).

| Call | Pattern | On a match |
|---|---|---|
| `link(subject)` (l.306) | `abnormal_termination()` (l.317): every cause but `Completed` | The caller is stopped with `PeerKilled`. Directed: `A link B` stops A when B dies, not the reverse. A subject that completes normally doesn't stop A (ticket 0053 T4, 2026-11-03; tests `link_does_not_fire_on_completed`, `link_fires_on_failed`) |
| `monitor(subject)` (l.328) | `any_termination()` (l.339): **terminal only** | `handle-actor-event(subject, event-type, data)` once, when the subject dies |
| `monitor-filtered(subject, pattern)` (l.350) | The caller's pattern | `handle-actor-event` for matching events only |

`link`, `monitor` and `monitor-filtered` return `err("no such actor")` or
`err("already terminated: <cause>")` when the runtime refuses the
subscription (above), and keep no local entry. When the subject's terminal
event arrives, the handler forgets that subject, so a later call on it asks
the runtime again. (Ticket 0053 T1, 2026-11-03.)

**To be told when an actor dies, use `monitor(subject)`.** Matching happens in
the host, so the watcher's wasm runs once, at the death. The cause is in
`data`, encoded as pack bytes. (Terminal-only since ticket 0053 T3,
2026-11-03; before that a bare `monitor` used `Pattern::any()`. Tests:
`plain_monitor_is_terminal_only`, `mutual_plain_monitors_stay_responsive` in
`theater-tests/tests/supervisor_actor_test.rs`.)

**The handler never waits on the watcher's wasm** (ticket 0053 T2,
2026-11-03). A drainer reads the handler's chain channel continuously,
matches, stops the caller for a link at once, and queues monitor deliveries;
a second loop makes the wasm calls. So a watcher can't hold up the actors it
watches. The queue holds 256 events (`DELIVERY_QUEUE_CAP`); past that,
non-terminal events are dropped and counted per subject, and the next
delivery for that subject is preceded by one with event-type
`monitor-dropped:<n>` and empty data. Terminal events are never dropped. Each
event is delivered once, however many of the caller's monitors on that
subject match it. `link` / `monitor` of the caller itself is
`err("cannot link/monitor self")`.

A broad `monitor-filtered` (e.g. `Pattern::any()`, the whole chain; the guest
preset is `theater_guest::filters::everything()`) still floods: every
matching event of the subject becomes a wasm call in the watcher. Two actors
that watch each other this way feed each other events without end (each
delivery adds events to the watcher's own chain). Before T2 they deadlocked on
the first call, once both channels were full (theater#235 §1.2); now they stay
responsive and drop the excess. Tests: `self_monitor_is_an_error`,
`mutual_whole_chain_monitors_stay_responsive_and_report_drops` in
`theater-tests/tests/supervisor_actor_test.rs`.

## 4. Known gaps

- `initiate_teardown` emits the terminal event on the runtime's command loop,
  and chain delivery is strict (§3.1), so one subscriber whose channel is full
  stalls every runtime command until it drains (rt:1094; checked at
  `c1941b60`, 2026-11-03). The lifecycle handler's channel no longer fills
  (its drainer, §3.3), so this is down to other subscribers: host-side ones
  such as a global subscription.
- A full mailbox or a slow channel accept stalls the router for everyone
  (§1.1, §1.4).
- Deferred request replies don't work, and request failures look like empty
  replies (§1.3).

## 5. Keeping this page true

When you change any behavior above, update this page in the same PR. When you
learn something about delivery or lifecycle by reading the source, add it here
with a file and line, and the commit you checked.

## 6. Permissions at spawn

Checked on branch `permissions-narrow-on-spawn` (ticket 0069, 2026-11-06).
Paths are under `crates/`; "rt" is `theater/src/theater_runtime.rs`.

- **An actor's set comes from its spawner.** With `parent_id` set (as
  `runtime.spawn` sets it), the actor's effective `HandlerPermission` is its
  spawner's, narrowed by its own `permission_policy`, and its handlers are
  copies of the spawner's own, not the host's (rt:883-934, rt:1284). An actor
  the host started (`parent_id: None`) gets root narrowed by its policy and
  the host's handlers. If the spawner is no longer running the spawn is
  refused (rt:929).
- **A `[[handler]]` block can only narrow.** For a spawned actor it is checked
  twice before anything is instantiated: against the effective set
  (`validate_manifest_permissions`, `theater/src/config/enforcement.rs`,
  called at rt:1297), and against the spawner's own handler config by
  `Handler::check_child_config` (`theater/src/handler/mod.rs:203`). filesystem
  requires the path, with symlinks resolved and no `..`, to be inside the
  spawner's root, and store the same for `base_path` (both through
  `check_path_within` in `theater/src/config/enforcement.rs`); http-client
  requires `allowed_hosts` to be a subset (`theater-handler-http-client/src/lib.rs:130`);
  process requires `allowed_programs` and `max_processes` within the
  spawner's, and no `inherit_env` the spawner lacks
  (`theater-handler-process/src/lib.rs:134`). Since ticket 0075
  (2026-11-07): udp requires `allowed_bind_ports`, `allowed_peers` (each
  entry starting with one of the spawner's prefixes) and
  `allowed_multicast_groups` within the spawner's and `max_sockets` no
  higher (`theater-handler-udp/src/lib.rs:424`); tcp requires
  `max_connections` no higher, `listen` and `idle_timeout_secs` unset or the
  spawner's, TLS `cert`/`key`/`ca_cert` only the spawner's own files and
  `skip_verify` only if the spawner skips (`theater-handler-tcp/src/lib.rs:110`);
  podman refuses any setting (`theater-handler-podman/src/lib.rs:73`);
  terminal requires the spawner's `raw_mode`
  (`theater-handler-terminal/src/lib.rs:208`). A replay block's `chain`
  must be a file inside the spawner's filesystem root, checked by
  `HandlerRegistry::check_child_path` before the runtime reads it
  (rt:1301, `theater/src/handler/mod.rs:224`); a spawner with no
  filesystem handler, or only a temporary root, admits none. Other
  handlers (random, timer, and the config-less ones) accept any config
  (the default `check_child_config`). A block that is left out inherits
  the spawner's config.
- **tcp has no address allowlist.** `TcpHandlerConfig::listen` is never
  read (`theater-handler-tcp/src/lib.rs:81`): `listen()` and `connect()` use
  whatever address the actor passes. `idle_timeout_secs` and
  `max_connections` live on state shared by every tcp actor in the runtime:
  `max_connections` is the host's and a manifest's value is ignored
  (`theater-handler-tcp/src/lib.rs:1124`); the last manifest to set
  `idle_timeout_secs` sets it for all. udp's peer check is a string prefix,
  so `"10.0.0.1"` also admits `10.0.0.10` (`theater-handler-udp/src/lib.rs:505`).
- **The set is fixed at spawn.** `set_permissions` is called once, when the
  actor's handlers are made (`theater/src/handler/mod.rs:188`). Nothing
  changes a running actor's set.
- **What is checked at call time:** filesystem, http-client (the grant's
  `allowed_hosts`/`allowed_methods` on top of the config's allowlist,
  `theater-handler-http-client/src/lib.rs:99`), process (grant's
  `allowed_programs`/`max_processes`, `theater-handler-process/src/lib.rs:180`)
  and runtime; udp checks its config's lists. message-server, store, tcp
  and rpc check no grant yet.
- **Restricting a list that is unrestricted** (`None`) gives the restriction
  (`intersect_options` in `theater/src/config/permissions.rs`). Before
  0069 it gave `None` (unrestricted), so a policy that limited hosts did nothing.

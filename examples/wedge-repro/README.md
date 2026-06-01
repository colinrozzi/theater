# wedge-repro

Minimal reproduction of theater's "no available capacity" wedge — the
production sentinel was hitting this on inbox traffic after 20-30 min of
uptime, with single misbehaving connections amplifying through the
supervisor → child recording chain.

## What it does

Two actors:

- **noisy-child** — on `init`, emits `LOG_BURSTS` (default 100,000)
  `theater:simple/runtime.log` calls in a tight loop. Each one becomes a
  chain event on its own chain.
- **supervisor** — spawns noisy-child and registers
  `theater:simple/supervisor-handlers.handle-child-event`. For every child
  event, theater calls into the supervisor and records the call as a
  `wasm-call` chain entry on the supervisor's chain, with the child event's
  payload embedded. That's the amplification mechanism.

Under the burst load, the runtime command channel saturates. Expected log
output (on theater's stderr, as INFO/WARN/DEBUG tracing):

```
WARN  Failed to send event notification: no available capacity
WARN  Failed to send event notification: no available capacity
... (hundreds-thousands)
```

Followed eventually by silent process exit if the burst is large enough.

## Building

```sh
cd noisy-child  && cargo build --release --target wasm32-unknown-unknown && cd ..
cd supervisor   && cargo build --release --target wasm32-unknown-unknown && cd ..
```

## Setup — edit one absolute path

Theater resolves the `package` field in a manifest relative to that
manifest's directory when loaded as a top-level actor, but resolves it
relative to theater's cwd when loaded via `supervisor.spawn`. For the
child manifest (loaded by `supervisor.spawn`), edit
`noisy-child/manifest.toml` and replace the absolute path with your
checkout location:

```toml
package = "/path/to/theater/examples/wedge-repro/noisy-child/target/wasm32-unknown-unknown/release/noisy_child.wasm"
```

(The default is the original author's path; works only if you have an
identical checkout location.)

## Running

From the `examples/wedge-repro` directory:

```sh
RUST_LOG=theater=info,theater_handler_supervisor=debug \
  theater spawn supervisor/manifest.toml > /tmp/wedge.log 2>&1 &
sleep 30
kill %1
grep -c 'no available capacity' /tmp/wedge.log
```

On the original author's machine, a 30-second run produces **~100,000**
"no available capacity" WARNs against a 100k event burst — confirming
amplification at 1:1 (each child event saturates one command channel slot
on the supervisor side).

## Tuning

Edit `noisy-child/src/lib.rs`:

- `LOG_BURSTS` — how many log events to emit on init. Larger → stronger
  guarantee the wedge fires. Smaller → useful for finding the threshold.

Edit `supervisor/src/lib.rs`:

- The `handle-child-event` body is intentionally a no-op. Adding work
  there (e.g. serializing state into the content store) only accelerates
  the wedge — the amplification we're studying happens in theater's
  recording path, not the actor body.

## Observed (2026-05-31 author run, theater 0.3.18)

| Metric | Value |
|---|---|
| `LOG_BURSTS` (child) | 100,000 |
| Wall time to emit all events | ~25-30s |
| "no available capacity" WARNs in that window | 99,972 |
| Did theater exit on its own? | **No** — required SIGTERM |
| Extra signal | 64 `ERROR Failed to decode result: BufferTooSmall { need: 4, have: 0 }` during the burst — supervisor's `handle-child-event` return-value decoding is failing too |

The warn count of ~99,972 against 100,000 events is roughly 1:1, so the
supervisor's command-channel slot is filled per child event. That's the
direct mechanism theater-dev's T1/T2/T3 are aimed at. The lack of
self-termination in this isolated repro vs. the 20-min wedge in prod
suggests the prod death involves additional pressure (a longer-lived
parent, allocator stalls under multi-GB chain memory, or the per-conn
spawn/teardown cycle) — worth characterizing in Phase 2.

## What this validates

This repro exercises the SAME runtime path that prod sentinel hit:

1. Child emits chain events at high rate
2. theater's supervisor handler dispatches `handle-child-event` to parent
3. theater records the dispatch as a `wasm-call` on parent's chain
4. The recording goes through the runtime command channel
5. Channel fills under sustained burst

Theater-dev's three-tier fix path
([T1/T2/T3 in chain/mod.rs:255 + recording layer](../../crates/theater/src/chain/mod.rs))
should make this scenario survivable. Use this directory as a regression
guard once a fix lands — promote the manual run into a `cargo test` in
`crates/theater-tests/tests/wedge_repro_test.rs`.

## Phase staging

- **Phase 1 (this PR)**: runnable repro artifact. Manual invocation.
- **Phase 2**: observability harness (memory curve, channel-fill rate,
  time-to-first-warn, time-to-death). Repeatable measurement.
- **Phase 3**: cargo integration test asserting fix prevents wedge.

See `theater/notes/wedge-investigation-2026-05-31.md` for the diagnosis +
prod inspection that motivated this.

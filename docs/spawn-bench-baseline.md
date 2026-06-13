# Spawn-bench baseline (theater 0.3.25, 2026-06-12)

First measurement using the new spawn-pipeline instrumentation
(`crates/theater`, `crates/theater-handler-supervisor`) and bench harness
(`examples/spawn-bench/`).

## Setup

- Theater commit: `main` at `pslqowls 3808d5bd` (release-20260608 == 0.3.25).
- Build: `cargo build -p theater-cli --bin theater --release`.
- Bench: `examples/spawn-bench/`, 50 sequential `supervisor.spawn` calls of
  `noop-child` (minimal child — only exports `init`, returns immediately).
- Single parent actor. No concurrent multi-parent pressure yet.
- Host: agentry sandbox, x86_64 linux.

Run command:

```sh
cd examples/spawn-bench
NO_COLOR=1 theater spawn ./supervisor/manifest.toml \
  --log-level info > /tmp/bench.log 2>&1
./analyze.sh /tmp/bench.log
```

## Per-phase percentiles (ms, n = 50 spawns + 1 supervisor)

| Phase                                    |   n |   min |   p50 |   p95 |   p99 |   max |
|------------------------------------------|----:|------:|------:|------:|------:|------:|
| supervisor.manifest_resolve              |  50 |     0 |     0 |     0 |     0 |     0 |
| supervisor.manifest_parse                |  50 |     0 |     0 |     0 |     0 |     0 |
| supervisor.wasm_resolve                  |  50 |     0 |     0 |     0 |     0 |     0 |
| supervisor.runtime_setup_and_init        |  50 |     8 |    10 |    11 |    12 |    12 |
| **supervisor.spawn_total**               |  50 |     9 |    10 |    11 |    12 |    12 |
| runtime.handler_registry                 |  51 |     0 |     0 |     0 |     0 |     0 |
| runtime.handler_setup (per handler × 10) | 510 |     0 |     0 |     0 |     0 |     0 |
| **runtime.pack_instance_new**            |  51 |     8 |     9 |    11 |    16 |    16 |
| runtime.metadata_and_verify              |  51 |     0 |     0 |     0 |     0 |     0 |
| runtime.cache_function_types             |  51 |     0 |     0 |     0 |     0 |     0 |
| runtime.spawn_handler_tasks              |  51 |     0 |     0 |     0 |     0 |     0 |
| runtime.build_actor_resources_total      |  51 |     8 |    10 |    11 |    16 |    16 |
| **runtime.register**                     |  51 |     8 |    10 |    11 |    16 |    16 |
| runtime.setup                            |  51 |     8 |    10 |    11 |    16 |    16 |
| runtime.init (detached)                  |  51 |     0 |     0 |     0 |   522 |   522 |

`runtime.register.elapsed_ms` is the per-spawn runtime-command-loop blocking
time — every millisecond there is one the loop spent serialized on this
spawn instead of draining other commands.

## Findings

1. **`runtime.pack_instance_new` is the wedge surface.** ~9 ms median,
   ~16 ms p99. Every other phase is sub-millisecond. This is wasmtime's
   compile + linker setup + instantiate, all in the runtime command loop's
   critical section. Everything else combined contributes < 1 ms.
2. **Per-spawn throughput cap ≈ 100 spawns/sec from a single supervisor**
   on this hardware, for this tiny child. Real-world actors (more imports,
   real init work) will pay more.
3. **`runtime.init` runs detached** — confirmed by p50 = 0 ms for the
   queue-blocking path (init is not in `runtime.register`'s window). The
   p99 = 522 ms outlier is one detached init that took a while; it did
   not block the command loop.
4. **Manifest fetch + parse, WASM fetch are not the bottleneck.** Local
   filesystem + OS cache after first read. Would matter more for `https://`
   manifests on cold caches; sentinel's caching layer covers that on the
   VPS.
5. **Per-handler `setup_host_functions_composite` is essentially free.**
   10 handlers per spawn × 50 spawns = 510 samples, all 0 ms. Whatever
   the wedge story turns out to be, it isn't linker wiring.

## Implications for the wedge investigation

- After PR #105/#108 (subscription opt-in landed in 0.3.25), chain-event
  amplification under TCP load is no longer the bottleneck for
  per-conn-child patterns. **Spawn cost moves to the front of the line.**
- For frontdoor's §6 per-conn-child design, the math: 1 acceptor × 10 ms
  per spawn = 100 conn/sec ceiling. A connection storm at >100 conn/sec
  will queue, and `theater_tx`'s default buffer (32, per the supervisor
  handler code) backs up.
- The compile cost is paid on every spawn even though the WASM bytes are
  identical across N spawns of the same child kind. **A wasmtime
  `Component`-level cache keyed by `wasm_bytes` hash is the most
  obvious lever.** That should drop `runtime.pack_instance_new` to
  near-zero on the warm path.
- A `wasmtime::InstancePre` cache would go one step further by
  precomputing the per-instantiate work. But the Component cache is the
  larger win on cold→warm.
- Pre-instantiated **warm pool** of a configured child kind could take
  the warm-path latency to ~0 at the cost of memory and start-up time.
  Probably overkill until we measure with the Component cache in place.

## Things this baseline does NOT measure

- **Multi-parent parallel load** — the bench is single-parent sequential.
  In a multi-parent workload the runtime command loop is the only
  serialization point; per-parent host-fn await drops out.
- **Real child cost** — noop-child returns immediately from init. Real
  actors with imports and init work pay more. Note `runtime.init` is
  detached so it doesn't show up in the queue-blocking number, but
  setup-side costs (handler registry construction, hash verification on
  more imports) do.
- **Cold vs warm filesystem** — the bench reads the same manifest and
  WASM file 50 times in immediate succession; the OS page cache is hot
  after the first read. Cold-disk numbers will be higher for
  `supervisor.manifest_resolve` / `supervisor.wasm_resolve`.
- **HTTPS-fetched manifests** — sentinel-fetch path adds network +
  signature verification. Not exercised here.

## Next measurements (suggested order)

1. Re-run with a real child (e.g. `wedge-repro/noisy-child` without the
   100k log burst, or a child with 3–4 handler imports). Quantifies how
   much non-noop init costs.
2. Multi-parent parallel: spawn the bench supervisor N times concurrently
   from the host CLI, each spawning M children. Surface the runtime
   command-channel queue depth under load. Will need either a small
   harness or N CLI invocations.
3. ~~Once a `Component` cache is prototyped: re-run this baseline, compare
   `runtime.pack_instance_new` distributions.~~ Done — see below.

## Compile cache results (engine sharing + module cache, 2026-06-12)

Same bench, same hardware, after the engine-sharing refactor
(theater #114) and the `CachingPackRuntime` module cache (SHA-256 of
wasm bytes → `wasmtime::Module`, via packr's `wrap_module` from pack #29).

Cache behavior across the run: **2 misses** (bench supervisor itself +
first noop-child), **49 hits**.

| Phase (ms)                          |   n |   min |   p50 |   p95 |   p99 |   max |
|--------------------------------------|----:|------:|------:|------:|------:|------:|
| supervisor.spawn_total               |  50 |     0 |   **0** |     0 |    11 |    11 |
| runtime.module_compile               |  51 |     0 |     0 |     0 |    14 |    14 |
| runtime.pack_instance_new            |  51 |     0 |   **0** |     0 |    14 |    14 |
| runtime.register (queue-blocking)    |  51 |     0 |   **0** |     0 |    15 |    15 |

Warm-path spawn drops from **~10 ms to sub-millisecond** (elapsed_ms=0
means <1 ms at this instrumentation resolution). The p99/max entries are
the two cold compiles — exactly the expected shape. The runtime command
loop's per-spawn serialization cost on the warm path is now negligible;
the spawn-rate ceiling moves from ~100/sec to wherever instantiate +
init-dispatch tops out (>1000/sec; finer-grained instrumentation needed
to measure precisely).

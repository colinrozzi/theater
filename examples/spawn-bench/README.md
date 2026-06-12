# spawn-bench

Measures actor spawn latency by phase. One parent (`spawn-bench-supervisor`)
fires `SPAWN_COUNT` sequential `supervisor.spawn` calls of `noop-child`.
The instrumentation added in `crates/theater` and
`crates/theater-handler-supervisor` emits one `info!` line per phase per
spawn, tagged `phase = supervisor.* | runtime.*` with an `elapsed_ms`
field. Aggregate by piping theater's stderr through the analyze script.

## Why sequential

The frontdoor §6 per-conn-child pattern has a single acceptor spawning a
child per inbound connection. That's one parent serializing on its own
host-fn await, plus N spawns serializing through the runtime command loop.
Sequential single-parent measurement isolates both effects without TCP
plumbing muddling the signal. Multi-parent parallel load is a follow-up
once we know what to optimize.

## Build

```sh
cd noop-child   && cargo build --release --target wasm32-unknown-unknown && cd ..
cd supervisor   && cargo build --release --target wasm32-unknown-unknown && cd ..
```

## Run

From the `spawn-bench/` directory (so the relative `./noop-child/manifest.toml`
in the bench supervisor resolves):

```sh
RUST_LOG=theater=info,theater_handler_supervisor=info \
  theater start ./supervisor/manifest.toml 2> bench.log
```

The supervisor's init fires all spawns in a loop and then logs
`[spawn-bench] done`. Wait for that line, then Ctrl-C theater.

## Phases emitted

Per spawn, in order of execution:

| Phase | Where it runs | What it covers |
|---|---|---|
| `supervisor.manifest_resolve` | Parent actor's host-fn (in supervisor handler) | Fetch manifest bytes (HTTP/file/store) |
| `supervisor.manifest_parse` | Parent's host-fn | TOML deserialize |
| `supervisor.wasm_resolve` | Parent's host-fn | Fetch WASM bytes |
| `supervisor.runtime_setup_and_init` | Parent's host-fn (blocked on `response_rx`) | Sum of all runtime-side work below |
| `supervisor.spawn_total` | Parent's host-fn | End-to-end host-fn wall time |
| `runtime.handler_registry` | Runtime command loop | Build per-spawn handler set from manifest |
| `runtime.handler_setup` | Runtime command loop (per handler) | `setup_host_functions_composite` |
| `runtime.pack_instance_new` | Runtime command loop | `PackInstance::new_with_interceptor` (compile + linker + instantiate) |
| `runtime.metadata_and_verify` | Runtime command loop | `get_metadata_with_hashes` + interface hash verification loop |
| `runtime.cache_function_types` | Runtime command loop | Per-export type cache |
| `runtime.spawn_handler_tasks` | Runtime command loop | `tokio::spawn` per handler |
| `runtime.build_actor_resources_total` | Runtime command loop | Sum of all setup work in `build_actor_resources` |
| `runtime.setup` | Runtime command loop | Wait-for-setup time (≈ build_actor_resources_total) |
| `runtime.register` | Runtime command loop | Insert into actor registry — last line before the loop frees |
| `runtime.init` | Detached tokio task | `actor.init` RPC (does not block the runtime loop) |

The wedge surface is everything between `runtime.handler_registry` and
`runtime.register` — that interval is when the runtime command loop is
serialized on a single spawn.

## Aggregating

`analyze.sh` reads stderr, extracts elapsed_ms per phase, prints
min/p50/p95/p99/max and n per phase:

```sh
./analyze.sh bench.log
```

Or quick eyeball:

```sh
grep 'phase=runtime.pack_instance_new' bench.log | grep -oE 'elapsed_ms=[0-9]+' | sort -t= -k2 -n | tail -5
```

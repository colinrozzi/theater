# Theater examples

A small, canonical set of actors — each one shows a single core capability of
the runtime. They target the current `packr-guest` (0.23) and the current
`theater:simple/*` interfaces.

| Example | Shows | Handlers used |
|---|---|---|
| [`hello`](hello) | The minimal actor: `init` + `self.log` | `self` |
| [`counter`](counter) | State + inter-actor messaging (`register` / `handle-send`) | `self`, `message-server` |
| [`supervisor`](supervisor) | Spawning a child and learning of its death via `handle-lifecycle-event` (auto-monitor) | `self`, `supervisor` |
| [`link`](link) | Fate-sharing between actors (`lifecycle.link` — subject dies ⇒ this actor stops) | `self`, `lifecycle` |
| [`store`](store) | Content-addressed storage: put / get / label | `self`, `store` |

## Building

Build them all (this is the guardrail that keeps them from rotting — an example
that drifts from the current interfaces stops compiling):

```bash
nix run .#build-examples
```

Or build one directly:

```bash
cd examples/hello && cargo build --release --target wasm32-unknown-unknown
```

## Running

```bash
cd examples
theater spawn hello/manifest.toml
```

The `supervisor` example spawns the `hello` example as its child, so build
`hello` first and run `supervisor` from inside `examples/` for the relative
manifest path to resolve.

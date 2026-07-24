# comp-composite — a packr multi-memory composite actor (e2e fixture)

`comp_actor_composite.wasm` is a **pre-built** packr composite: two isolated
components fused into one multi-memory module by `packr compose` (packr's
Component-Model equivalent). It exists to prove a composite runs as a real
theater actor — see `crates/theater/tests/composed_actor_e2e_test.rs`.

It is committed as a binary because `packr compose` is (as of this writing)
unreleased packr tooling that theater's pinned `packr` crate does not expose, so
theater cannot compose it at test time. Regenerate it from the packr repo when
the composition transform changes.

## What it is

- **Entry component** (`packages/comp-actor` in the packr repo): exports
  `theater:simple/actor.init`, imports `math.double` from the provider and the
  residual host import `theater:simple/runtime.log`. `init` calls `double(21)`
  across the component gap and returns state `{ doubled: 42 }`.
- **Provider component** (`packages/math-real` in the packr repo): exports
  `double(n) = n * 2`.

The composite keeps the two components in **separate memories**; the
`math.double` call is internalized by a bridging shim, so the only residual
import is `theater:simple/runtime.log`, which theater supplies at instantiate.

## Regeneration

From the packr repo (`/home/colin/work/pack`):

```sh
# 1. Build the two component wasms (self-contained: export memory, no entry).
RUSTFLAGS="-C link-arg=--export-memory -C link-arg=--no-entry" \
  cargo build --release --target wasm32-unknown-unknown \
  --manifest-path packages/comp-actor/Cargo.toml
RUSTFLAGS="-C link-arg=--export-memory -C link-arg=--no-entry" \
  cargo build --release --target wasm32-unknown-unknown \
  --manifest-path packages/math-real/Cargo.toml

# 2. Compose (manifest lists the two components + the math.double -> double link).
cargo run --release --bin packr -- compose <manifest>.toml -o comp_actor_composite.wasm
```

Manifest:

```toml
[[component]]
name = "app"
wasm = "packages/comp-actor/target/wasm32-unknown-unknown/release/comp_actor.wasm"
entry = true

[[component]]
name = "math"
wasm = "packages/math-real/target/wasm32-unknown-unknown/release/math_real.wasm"

[[link]]
consumer = "app"
import = "math.double"
provider = "math"
export = "double"
```

Then copy the output here. The packr-side mirror of this e2e (compose + drive
`actor.init` under packr's own runtime) is `tests/compose_actor.rs` in the packr
repo.

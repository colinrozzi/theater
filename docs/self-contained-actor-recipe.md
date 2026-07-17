# Self-contained actor recipe (packr 0.10.2)

The canonical, copy-me reference for building a fleet actor against the packr
**0.10.2 self-contained** model (PIC removed). Worked example: `test-actors/state-test`
(a single-package message/runtime actor), driven end-to-end in CI by the
**Reference actor (self-contained e2e)** job in `.github/workflows/ci.yml`.

## Why a composite

theater's 0.10.x loader does `assert_self_contained`. A bare cargo-built member
(imported memory, unresolved `pack:alloc`, `__pack_types` surface) is **not**
self-contained and **will not load**. The deployable artifact is a
**self-contained composite** = the actor member + the packr bundled allocator,
fused so memory + `pack:alloc` are internalized and only the host
`theater:simple/*` imports remain residual. `theater build --release` produces
and verifies it in one command — no Rust link harness.

## 1. `Cargo.toml`

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
packr-guest = "0.10.2"        # NO features — the `pic` feature is gone

[profile.release]
opt-level = "s"
lto = true                    # keep the member small
```

## 2. `.cargo/config.toml` — the fixed-base recipe

Replaces the old PIC flags entirely (no `relocation-model=pic`,
`--experimental-pic`, `-shared`, `--export=__wasm_call_ctors`):

```toml
[target.wasm32-unknown-unknown]
rustflags = [
    "-C", "link-arg=--import-memory",
    "-C", "link-arg=--initial-memory=8388608",
    "-C", "link-arg=--stack-first",
    "-C", "link-arg=-zstack-size=262144",
    "-C", "link-arg=--global-base=327680",
    "-C", "link-arg=--no-entry",
    "-C", "link-arg=--no-merge-data-segments",
]
```

- Non-PIC, built at a fixed absolute base so data needs no relocation.
- `--global-base=327680` (`0x50000`) = the first package slot for a
  **single-package** actor. (Multi-package actors — more than one member fused
  with `[[link]]` edges — need a different base per member per packr's `Layout`;
  none of the current fleet actors do this. Flag theater-dev if yours does.)
- `--no-merge-data-segments` keeps the CGRF `__pack_types` surface as its own
  segment (starting with the magic) so packr's `read_surface` can find it;
  wasm-ld would otherwise bury it in `.rodata`.

## 3. Handler ABI

Flat/positional (0.8+): `fn handler(state, ..params)` — each pact param is its
own `Value` arg, and `fn init(state)`. Not a wrapped `(state, params)` tuple.

## 4. Build (compose + verify in one command)

```sh
theater build --release <actor-dir>
```

This runs `cargo build --target wasm32-unknown-unknown --release`, then links the
member + `packr::DEFAULT_ALLOCATOR_WASM` into `<name>.composite.wasm`, verifies
it (below), and rewrites `manifest.toml` `package = …` to point at the composite.
Requires **`wasm-merge`** (binaryen) and **`wasm-tools`** on PATH — both are in
the theater dev shell (`nix develop`). `--no-compose` emits the bare member only
(not loadable; debugging).

### Crane / cargo-workspace builds → `theater compose`

`theater build` re-runs `cargo` and assumes a **standalone crate** (its own
`target/` dir). It does **not** fit a crane flake or a cargo **workspace member**
(whose `target/` is at the workspace root). For those, build the member however
you already do (crane `buildPackage`, offline/sandboxed cargo, …), then compose
the **prebuilt member** with the standalone subcommand:

```sh
theater compose <member.wasm> [-o <name>.composite.wasm]     # verifies by default; --no-verify to skip
```

It links the member + the bundled allocator (single-package, base `0x50000`),
runs the same verification, writes the composite, and prints its path to stdout.
Same PATH tools (`wasm-merge`, `wasm-tools`). Typical crane `installPhase`:
crane builds the bare members → `theater compose` each → install the composites.
Deploy the composite, never the bare member.

## 5. Verify (done automatically by `theater build`; manual check)

```sh
wasm-tools validate <name>.composite.wasm
# every residual import must be a host theater:simple/* function:
wasm-tools print <name>.composite.wasm | grep '(import' | grep -v 'theater:simple/'   # must print NOTHING
```

Any `(import "env" "memory" …)`, `pack:alloc`, or `__linear_memory` means the
allocator/memory was not internalized (a bare member, or the fixed-base recipe
was not applied). `theater build` fails the build on this.

## 6. Run

```sh
theater spawn <actor-dir>/manifest.toml      # loads the composite (assert_self_contained) + runs init
```

`theater spawn` creates the runtime in-process, loads the composite through the
0.10.x self-contained loader, and calls `actor.init`. If the composite is not
self-contained, it fails here with a load error rather than starting.

## Crane / Nix notes (if building actors under crane)

- **`CARGO_ENCODED_RUSTFLAGS`**: crane does not reliably pick up
  `.cargo/config.toml` rustflags — pass the fixed-base flags from §2 via
  `CARGO_ENCODED_RUSTFLAGS` (0x1f-separated) in the actor's crane derivation.
- **`cargoArtifacts = null`** for the wasm member build (don't share host
  `cargoArtifacts` into the `wasm32-unknown-unknown` build).
- **`lto = true`** stays (member size; §1).
- **New vs 0.8.1 PIC**: the build environment needs **binaryen** (`wasm-merge`,
  for `packr::link`) and **wasm-tools** (verify). PIC needed no external linker.

## Deploy model

The 0.8.1→0.10.2 cutover is one atomic per-process flip (the theater binary +
all its actors together; mixed won't load). After cutover, self-contained
relaxes to **per-actor** deploys: a single actor's composite can be rebuilt and
redeployed without touching the binary or sibling actors, as long as the
`theater:simple/*` host pact ABI and the packr-abi wire format are unchanged.

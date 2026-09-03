# Open handler registry — Design

**Status:** proposal (design; not yet implemented)
**Date:** 2026-09-03

## Problem

A handler's config is defined in **core**, not in the crate that implements the
handler. Concretely, `crates/theater/src/config/actor_manifest.rs` holds a closed
enum plus ~25 config structs:

```rust
#[serde(tag = "type")]
pub enum HandlerConfig {
    Tcp { config: TcpHandlerConfig },
    Store { config: StoreHandlerConfig },
    // … one variant per handler, every config struct defined here …
}
```

Adding a handler means editing core in **three** places:

1. a new `HandlerConfig` variant,
2. a new config struct in core's `actor_manifest.rs`,
3. a new match arm in core's `enforcement.rs` (`validate_manifest_permissions`
   matches every variant).

A handler crate then imports its *own* config back from core
(`use theater::config::actor_manifest::TcpHandlerConfig`). And a **third party
cannot add a handler at all** — you can't extend a closed enum from outside its
crate. As we grow the handler set (and invite others to write handlers), this is
the wall.

## What's already right

The `Handler` trait is **already open** — its doc even says "external handler
crates can implement this trait and register their handlers without depending on
the concrete enum." Two mechanisms already exist:

- **Registration is a `Vec<Box<dyn Handler>>`** in `HandlerRegistry`; anyone can
  `register(MyHandler)`.
- **Config is matched to a handler by name**, not by enum position:
  `configs.iter().find(|c| c.handler_name() == handler.name())` in
  `clone_with_configs`.

So the *only* thing forcing the closed enum is the **type** flowing through one
method:

```rust
fn create_instance(&self, config: Option<&HandlerConfig>) -> Box<dyn Handler>;
```

Every handler receives the whole closed enum and pattern-matches its own variant
out. Break that one coupling and the enum can go.

## Target design

### 1. Config becomes generic at the manifest boundary

The manifest parses each `[[handler]]` block into a **tag + raw table**, not a
typed variant:

```rust
pub struct RawHandlerConfig {
    pub type_: String,       // the `type = "tcp"` tag
    pub raw: toml::Table,    // the remaining fields, un-deserialized
}
```

Core stops owning per-handler config types entirely. The `HandlerConfig` closed
enum and all ~25 structs leave core.

### 2. The handler owns its config (parse + validate)

The `Handler` trait gains config ownership. `name()` already returns the tag, so
it doubles as `type_tag()`. Two methods change/appear:

```rust
trait Handler {
    fn name(&self) -> &str;                    // the manifest `type` tag — exists today

    /// Deserialize THIS handler's typed config (defined in the handler crate)
    /// from the raw table, and build a per-actor instance.
    fn create_instance(&self, raw: Option<&toml::Table>) -> Result<Box<dyn Handler>>;

    /// Validate this handler's config against the actor's granted permissions.
    /// Moved out of core's enforcement.rs — the per-handler knowledge lives with
    /// the handler; core still calls it for EVERY handler (see §4).
    fn validate(&self, granted: &HandlerPermission) -> Result<(), PermissionError> {
        Ok(()) // default: no capability to check
    }

    // set_permissions / init / run / setup_host_functions_composite unchanged
}
```

Each handler crate defines its own `TcpHandlerConfig` (etc.), deserializes it from
the raw table in `create_instance`, and checks it against the grant in `validate`.

### 3. The registry is a factory map keyed by tag

`HandlerRegistry` already holds registered template handlers; matching stays by
name. `clone_with_configs` becomes: for each registered handler, find the
`RawHandlerConfig` whose `type_` equals `handler.name()`, hand it the raw table,
call `create_instance`. A manifest `type` that matches **no** registered handler
is an error — *you cannot configure a capability the node does not provide*, which
is exactly the property we want.

("Factory" is the right word **here** — each registered handler is the factory for
its own instances, keyed by tag. It is deliberately *not* reused as the name of
the composition crate; see §5.)

### 4. Validation: logic moves out, the loop stays in core

Today `enforcement.rs` centralizes config-vs-permission checks by matching every
enum variant — one auditable place proving every capability config is checked.
We keep that guarantee without keeping the logic in core: core keeps the
**uniform loop** (call `handler.validate(granted)` for every handler in the
registry), while each handler supplies its own check. It remains provably true
that *all* handlers are validated; the per-handler logic just lives with the
handler.

> **Trust note (decided):** security-relevant validation now ships in handler
> crates, including eventual third-party ones. The composition root (§5) chooses
> which handlers are registered at all, so the trust boundary is "which handlers
> does this node load" — an explicit, readable list — not "what got linked."

### 5. `theater-stage` — the composition root

The runtime is a library; something must name the concrete capability set and
boot a runnable instance. Today that assembly is hand-rolled in
`theater-cli/spawn.rs` **and copy-pasted into four theater-tests files**. It
belongs in one crate — **`theater-stage`** (the stage actors perform on):

```rust
// theater-stage
pub fn standard_handlers(theater_tx, …) -> HandlerRegistry { /* register the std set */ }

Stage::builder()
    .with_standard_handlers()
    .with(AcmeHandler::default())   // third-party, no core edit
    .build()
    .run()
    .await;
```

`theater-stage` depends on `theater` + every standard `theater-handler-*` crate,
and is the single readable answer to "what is a standard theater node." Then:

- **theater-cli** becomes clap commands over `theater-stage` (no hand-rolled
  registry).
- **theater-tests** call `theater_stage::standard_handlers()` instead of
  duplicating `full_registry` four times.
- the **console / control-plane actor** boots on the same crate.
- a **third party** uses `theater-stage` + `.with(their handler)`, or writes their
  own composition crate from `theater` + a curated set.

Explicit registration (not `inventory`/`typetag` link-time magic) is the chosen
model: for a capability runtime, "which host capabilities are loaded" must be a
list you can read, not an emergent property of linking.

### Final layering

```
theater                 mechanism: TheaterRuntime, Handler trait, registry
theater-store, …        shared primitives
theater-handler-*       capabilities — each owns its config + create_instance + validate
theater-stage           composition root: names the standard set, builds + runs
theater-cli / console / UX + embedders (thin)
theater-tests
```

`theater` knows zero concrete handlers; `theater-stage` is the one place that says
what a node is; the CLI is a face on it — the same "mechanism in the middle,
policy at the edges" shape as ripping out the server and pushing state into
modules.

## Migration — a bridge, not a big bang

The change touches the `Handler` trait, the registry, manifest parsing,
enforcement, and ~19 `HandlerConfig::` construction sites. Do it incrementally:

1. **Add the generic path alongside the enum.** Introduce `RawHandlerConfig` +
   the new `create_instance`/`validate` signatures, and a registry lookup by tag,
   while the closed enum still exists. Manifest parsing accepts both.
2. **Migrate handlers one at a time.** For each handler: move its config struct
   into its crate, implement `create_instance(raw)` + `validate`, drop its
   `HandlerConfig` variant and its `enforcement.rs` arm.
3. **Stand up `theater-stage`** and repoint the CLI + tests onto
   `standard_handlers()`.
4. **Delete the closed `HandlerConfig` enum** and core's per-handler structs once
   the last handler is migrated.

Each step is independently green.

## Open questions

- **Raw representation.** `toml::Table` is natural for TOML manifests but awkward
  for programmatic construction (tests). Alternatives: `serde_json::Value`, or a
  small `RawConfig` newtype with typed builders provided by `theater-stage`.
  Leaning `toml::Table` + stage-provided builders for tests.
- **Where does `ManifestConfig` live?** The manifest *shell* (name, package,
  permission policy) stays in core; only the per-handler config leaves. Confirm
  the split point (e.g. `permission_policy` stays; handler blocks go generic).
- **`create_instance` fallibility.** It returns `Result` now (deserialization can
  fail). The registry surfaces a bad handler config as a spawn error with the tag
  named.

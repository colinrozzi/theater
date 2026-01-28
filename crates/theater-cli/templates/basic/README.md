# {{project_name}}

A Theater actor using the Pack runtime.

## Building

Build the actor to WebAssembly:

```bash
# Fetch WIT dependencies (first time only)
wkg wit fetch

# Build the actor
cargo build --target wasm32-unknown-unknown --release
```

The output will be at `target/wasm32-unknown-unknown/release/{{project_name_snake}}.wasm`.

## Running

Run the actor with Theater:

```bash
theater start manifest.toml
```

## Development

This actor uses Pack's import/export macros for type-safe WASM interfaces:

- `#[import(wit = "...")]` - Import host functions
- `#[export(wit = "...")]` - Export actor functions

The actor implements `theater:simple/actor` and imports `theater:simple/runtime`.

See the [Pack documentation](https://github.com/colinrozzi/pack) for more details.

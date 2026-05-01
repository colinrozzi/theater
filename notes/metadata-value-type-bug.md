# Metadata Bug: `value` type reports as `option`

## Problem

When an actor declares `func(state: value)` in `pack_types!`, the wasm metadata reports the first parameter type as `option` instead of `value`. This causes theater's contract validation to reject values that don't match `option`.

## Where it manifests

- `pack_bridge.rs` — `validate_value_in_type_space` checks the store state against the function's first param type from wasm metadata
- Error: `State type mismatch for 'theater:simple/actor.init': expected option, got tuple<0>`

## What we know

- `pack_types!` macro generates metadata embedded in the wasm binary as `__PACK_TYPES_DATA`
- The `value` type is encoded as variant tag 20 in the metadata encoding (`metadata.rs:170`)
- The decoding side correctly maps tag 20 to `Type::Value` (`metadata.rs:937`)
- But at runtime, the decoded type comes back as `option`, not `value`
- Both `packr-guest` 0.4.0 and 0.5.0 exhibit this behavior
- The `packr inspect` command fails on the wasm with "Trailing bytes" error, so we can't easily inspect the raw metadata

## Where to investigate

1. **pack_types! macro expansion** — check what `__PACK_TYPES_DATA` actually contains (the byte array in the expanded code)
2. **Metadata encoding in pack-guest-macros** — `crates/pack-guest-macros/src/metadata.rs` — verify `TypeDesc::Value` encodes correctly
3. **Metadata decoding in pack** — `src/metadata.rs` `decode_type_collecting` — verify the bytes are read correctly
4. **The "Trailing bytes" error** — `packr inspect` can't read the metadata, which suggests the encoding format may have diverged between guest-macros and the host decoder

## Current workaround

- Theater passes `Value::Option { value: None }` as initial state instead of `Value::Tuple(vec![])`
- Actor init functions accept `Value` and manually handle `Option(None)` for fresh init vs existing state for restart
- The actor also has to unwrap from a Tuple wrapper since the export macro passes the full input as a single Value

## Related files

- `pack/crates/pack-guest-macros/src/metadata.rs` — metadata encoding
- `pack/src/metadata.rs` — metadata decoding + validation
- `theater/crates/theater/src/pack_bridge.rs` — where validation happens
- `theater/crates/theater/src/theater_runtime.rs:782` — default initial state

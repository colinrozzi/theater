# Changelog

## v0.3.0 (2026-04-26)

### Added
- **Host-side type contract enforcement**: runtime validates actor state and return types against declared signatures before and after every WASM call.
- **`FunctionTypeInfo` cache**: parameter and result types cached at actor startup for fast validation.
- **`pack_types!` type definitions**: actors can declare records, variants, enums in their type metadata.
- **External `.pact` files**: `pack_types!(file = "types.pact")` loads types from external files.
- **Contract enforcement docs**: `docs/contract-enforcement.md`.
- **Test actors**: `contract-test` (rich types), `pact-contract-test` (external .pact file, todo list).

### Changed
- Pack dependency pinned to `v0.2.0` via GitHub git tag.
- Cargo.toml uses git dependencies (no more `path = "../pack"`).
- `.cargo/config.toml` for local dev overrides (gitignored).
- Nix flake input from `github:colinrozzi/pack/v0.2.0`.
- `state-test` actor updated with typed record in `pack_types!`.

### Removed
- `deploy-docs.yml` CI workflow (was failing).
- Stale WASI documentation (implementing-wasi-handlers.md, wasi-http-design.md).

## v0.2.1

Previous release. Typed actor state, pack encoding for chain events, supervisor handler export checking, shutdown fixes.

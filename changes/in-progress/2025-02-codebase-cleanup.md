# Codebase Cleanup - February 2025

Post-pact-migration cleanup to improve code quality, fix broken tests, and remove dead code.

## Tasks

### 1. Fix Ignored Doc Tests
**Status**: Not Needed
**Files**: Various in `crates/theater/src/`

These 8 doc tests are intentionally `ignore`d - they're pseudo-code showing usage
patterns, not runnable examples. This is correct and doesn't need fixing.

### 2. Fix Ignored Integration Tests
**Status**: Blocked (needs runtime changes)
**Files**: `crates/theater/tests/`, `crates/theater-handler-tcp/tests/`

Investigation revealed deeper issues:
- `test_multi_handler_composite` - async host functions deadlock in standalone test context
  (works fine in full actor runtime). Needs wasmtime async executor investigation.
- `test_tcp_echo_and_chain` - actor init not called automatically by TheaterRuntime.
  Need to add `TheaterCommand::CallActorInit` or similar mechanism.

**Changes made:**
- tcp-echo actor now imports `listen` and calls it in init
- Test manifest passes listen address via initial_state
- Both tests remain ignored with updated explanations

### 3. Add TCP Replay Test
**Status**: Complete
**Files**: `crates/theater-replay-experimenting/`, `crates/theater/src/`

Created `run_tcp_replay_verification()` and `test_tcp_replay_verification` test.
Test now passes after implementing initial_state support.

**Fix Applied:**
Passed initial_state from manifest through the spawn chain:
- Added `initial_state: Option<String>` field to ManifestConfig
- Extract initial_state in spawn_actor (theater_runtime.rs)
- Pass through ActorRuntime::start -> build_actor_resources -> ActorStore::new
- ActorStore now initializes with the manifest's initial_state

**Changes made:**
- Added `get_tcp_echo_wasm_path()` helper
- Added `create_tcp_recording_manifest()` and `create_tcp_replay_manifest()`
- Added `create_tcp_registry()` helper
- Added `run_tcp_replay_verification()` function
- Added `test_tcp_replay_verification` test (now passing!)
- Updated main() to include TCP replay verification
- Updated tcp-echo actor with listen import
- Added initial_state field to ManifestConfig
- Threading initial_state through spawn chain

### 4. Remove Dead Scaffolding from pack-guest-macros
**Status**: Complete
**Files**: `pack/crates/pack-guest-macros/src/`

Removed unused scaffolding code:
- [x] `get_world_exports` function (removed)
- [x] `get_world_imports` function (removed)
- [x] `collect_exports` function (removed)
- [x] `ExportInfo` struct (made private, unused fields annotated)
- [x] `World.name` field (kept for API but annotated)
- [x] `ParseError.span` field (kept for API but annotated)
- [x] `WitValidationResult.function` field (annotated, reserved for future use)
- [x] `has_export_function` method (removed)

### 5. Fully Remove WIT+ Parser
**Status**: Deferred
**Files**: `pack/src/parser/wit.rs`

Investigation shows the WIT+ parser types are still used:
- `Interface` type used by `pack/src/runtime/interface_check.rs` for WASM validation
- Types re-exported from `pack/src/lib.rs` for API stability
- Already hidden with `#[doc(hidden)]` on the parser functions

To fully remove, would need to:
- [ ] Refactor `interface_check.rs` to use `Arena` instead of `Interface`
- [ ] Remove `wit.rs` module
- [ ] Remove legacy types from `parser/mod.rs`
- [ ] Update tests

**Decision**: Defer until Arena migration is needed elsewhere. Current state is acceptable.

### 6. Documentation
**Status**: Complete
**Files**: Various

- [x] Document the pact interface definition format
- [x] Update building-actors.md to reflect pact migration
- [x] Add examples for `pack_types!` macro usage

**Changes made:**
- Created `book/src/development/concepts/pact-interfaces.md` - comprehensive Pact documentation
- Updated `book/src/SUMMARY.md` - added link to Pact docs
- Rewrote `book/src/development/building-actors.md` - replaced WIT/bindings examples with pack_types!/pack_guest patterns

## Progress Log

### 2025-02-23
- Created cleanup tracking document
- Identified 6 areas for cleanup
- Investigated integration tests - found async executor issues and missing init call
- Updated tcp-echo actor with listen import (will work once init mechanism is added)
- Removed dead scaffolding from pack-guest-macros (4 functions, cleaned up struct annotations)
- Investigated WIT+ parser removal - deferred (still used by interface_check.rs)
- Created TCP replay test infrastructure (test ignored pending timing fix)
- Created pact-interfaces.md documentation
- Rewrote building-actors.md with modern pack_types!/pack_guest patterns
- Updated SUMMARY.md with Pact documentation link
- Investigated TCP replay test failure - found manifest initial_state not passed to actor store
- Implemented initial_state support: ManifestConfig field + spawn chain threading
- TCP replay test now passes with deterministic hash verification

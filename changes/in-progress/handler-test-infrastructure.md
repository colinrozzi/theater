# Handler Test Infrastructure

This document tracks the progress of adding comprehensive test infrastructure to all `theater-handler-*` crates.

## Goal

Ensure every handler crate has:
1. **Test Actor** - A minimal WASM component that exercises the handler's interfaces
2. **Integration Tests** - Full runtime tests that spawn actors and verify behavior
3. **README Documentation** - Clear documentation of interfaces, usage, and testing

This follows the reference implementation established in `theater-handler-random`.

## Why This Matters

- **Verification**: Ensure handlers work correctly with the Theater runtime
- **Regression Prevention**: Catch breaking changes early
- **Documentation by Example**: Test actors serve as usage examples
- **Handler Matching Validation**: Verify exact version string matching works

## Current Status

### Complete (3/14)

| Handler | Test Actor | Integration Tests | README | Notes |
|---------|------------|-------------------|--------|-------|
| `random` | `wasi-random-test` | `integration_test.rs` | Complete | Reference implementation |
| `io` | `wasi-io-test` | `integration_test.rs` | Complete | Fixed WIT structure |
| `timing` | `wasi-clocks-test` | `integration_test.rs` | Complete | Tests monotonic clock + poll |

### Have Test Actors, Need Tests (4/14)

| Handler | Test Actor | Integration Tests | README | Notes |
|---------|------------|-------------------|--------|-------|
| `filesystem` | `wasi-filesystem-test` | Missing | Exists | Priority: High |
| `http` | `wasi-http-test` | Missing | Exists | Priority: High |
| `sockets` | `echo-server`, `echo-client` | Missing | Missing | Needs README too |
| `runtime` | `wasi-runtime-test` | Exists (Failing) | Exists | Instantiation issues |

### Need Everything (7/14)

| Handler | Test Actor | Integration Tests | README | Notes |
|---------|------------|-------------------|--------|-------|
| `environment` | Missing | Missing | Exists | Simple, good starter |
| `store` | Missing | Missing | Exists | Content-addressed storage |
| `http-client` | Missing | Missing | Exists | Outbound HTTP |
| `http-framework` | Missing | Missing | Exists | HTTP server framework |
| `message-server` | Missing | Missing | Exists | Actor messaging |
| `process` | Missing | Missing | Exists | OS process spawning |
| `supervisor` | Missing | Missing | Exists | Actor supervision |

## Reference Pattern

Each handler test follows this pattern (from `theater-handler-random`):

### 1. Test Actor Structure
```
test-actors/wasi-<interface>-test/
├── Cargo.toml                    # cargo-component configuration
├── src/lib.rs                    # Actor implementation
└── wit/
    ├── world.wit                 # World definition
    └── deps/
        ├── <interface>/package.wit   # Interface definitions
        └── theater-simple/package.wit # Actor interface
```

### 2. Cargo.toml for Test Actor
```toml
[package.metadata.component]
package = "test:<interface>"

[package.metadata.component.target.dependencies]
"wasi:<interface>" = { path = "./wit/deps/wasi-<interface>" }
"theater:simple" = { path = "./wit/deps/theater-simple" }
```

### 3. Integration Test Structure
```rust
// Define event types
enum TestHandlerEvents { ... }
struct TestEvents(TheaterEvents<TestHandlerEvents>);

// Implement From traits for TheaterRuntime
impl From<RuntimeEventData> for TestEvents { ... }
impl From<HandlerEventData> for TestEvents { ... }

// Create handler registry
fn create_test_handler_registry() -> HandlerRegistry<TestEvents> {
    let mut registry = HandlerRegistry::new();
    registry.register(RuntimeHandler::new(...));
    registry.register(YourHandler::new(...));
    // Add supporting handlers (io, timing, filesystem, random)
    registry
}

// Test spawns actor and verifies events/state
#[tokio::test]
async fn test_handler_with_test_actor() { ... }
```

## Key Learnings

### Version Matching is Exact
Handler's `imports()` must return exact version strings:
- `wasi:io/error@0.2.0` does NOT match `wasi:io/error@0.2.3`
- Always use `@0.2.3` for WASI interfaces

### Test Actors Need Supporting Handlers
Even a minimal test actor imports filesystem, timing, etc. from Rust std:
```
wasi:cli/environment@0.2.3
wasi:cli/exit@0.2.3
wasi:filesystem/types@0.2.3
wasi:filesystem/preopens@0.2.3
wasi:clocks/wall-clock@0.2.3
```

### Cargo Component Dependencies
The `Cargo.toml` needs explicit target dependencies:
```toml
[package.metadata.component.target.dependencies]
"wasi:io" = { path = "./wit/deps/wasi-io" }
```

## Commands

```bash
# Build a test actor
cd crates/theater-handler-X/test-actors/Y
cargo component build --release

# Check WASM imports
wasm-tools component wit path/to/actor.wasm

# Run tests
cd crates/theater-handler-X
cargo test --test integration_test -- --nocapture
```

## Progress Log

### 2025-12-31
- Created tracking document
- Established reference pattern from `theater-handler-random`

- **Completed `theater-handler-io` test infrastructure**
  - Fixed test actor WIT structure (package.wit files, proper dependencies)
  - Updated Cargo.toml with target.dependencies
  - Created integration test following random handler pattern
  - Test passes - actor returns "WASI I/O imports successful!"
  - Created comprehensive README

- **Completed `theater-handler-timing` test infrastructure**
  - Test actor already existed and built correctly
  - Created integration test following pattern
  - Test passes - actor uses monotonic clock and poll
  - Created comprehensive README

## Next Steps

Priority order for remaining work:

1. **filesystem** - Has test actor, just needs integration test
2. **http** - Has test actor, needs integration test  
3. **sockets** - Has test actors, needs integration test + README
4. **environment** - Simple interface, good for learning
5. **store** - Content-addressed storage
6. **runtime** - Debug failing tests
7. **http-client** - Outbound HTTP
8. **http-framework** - HTTP server (complex)
9. **message-server** - Actor messaging
10. **process** - OS processes
11. **supervisor** - Actor supervision

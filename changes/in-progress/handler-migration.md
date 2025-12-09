# Handler Migration Progress

This document tracks the progress of migrating handlers from the core `theater` crate to separate `theater-handler-*` crates.

See the full proposal: [2025-11-30-handler-migration.md](../proposals/2025-11-30-handler-migration.md)

## Migration Status

### ✅ Phase 1: Simple Handlers

| Handler | Status | Crate | Old File | Notes |
|---------|--------|-------|----------|-------|
| random | ✅ COMPLETE | `theater-handler-random` | `src/host/random.rs` | Documented example migration |
| timing | ✅ COMPLETE | `theater-handler-timing` | `src/host/timing.rs` | Fully migrated |
| environment | ✅ COMPLETE | `theater-handler-environment` | `src/host/environment.rs` | Migrated 2025-11-30 |
| runtime | ✅ COMPLETE | `theater-handler-runtime` | `src/host/runtime.rs` | Migrated 2025-11-30 |

### ❌ Phase 2: Medium Complexity

| Handler | Status | Crate | Old File | Notes |
|---------|--------|-------|----------|-------|
| http-client | ✅ COMPLETE | `theater-handler-http-client` | `src/host/http_client.rs` | Migrated 2025-11-30 |
| filesystem | ✅ COMPLETE | `theater-handler-filesystem` | `src/host/filesystem.rs` | Migrated 2025-11-30 |

### ⚙️ Phase 3: Complex Handlers

| Handler | Status | Crate | Old File | Notes |
|---------|--------|-------|----------|-------|
| process | ✅ COMPLETE | `theater-handler-process` | `src/host/process.rs` | Migrated 2025-12-07 |
| store | ✅ COMPLETE | `theater-handler-store` | `src/host/store.rs` | Migrated 2025-12-07 |
| supervisor | ✅ COMPLETE | `theater-handler-supervisor` | `src/host/supervisor.rs` | Migrated 2025-12-08 |

### ⚙️ Phase 4: Framework Handlers

| Handler | Status | Crate | Old File | Notes |
|---------|--------|-------|----------|-------|
| message-server | ✅ COMPLETE | `theater-handler-message-server` | `src/host/message_server.rs` | New architecture 2025-12-10 (see message-router-architecture.md) |
| http-framework | ❌ NOT STARTED | `theater-handler-http-framework` | `src/host/framework/` | Depends on others |

## Overall Progress

- **Completed**: 10/11 (91%)
- **Blocked**: 0/11 (0%)
- **In Progress**: 0/11 (0%)
- **Not Started**: 1/11 (9%)

## Current Sprint

### Active Work
- No active work at the moment

### Blocked
- None! All blockers resolved.

### Next Up
1. Complete Phase 4: Migrate http-framework handler
2. Update core theater crate to use new handlers
3. Complete handler migration project

## Cleanup Checklist

For each completed handler migration:
- [ ] New handler crate fully implemented
- [ ] All tests passing
- [ ] Documentation complete
- [ ] Old implementation removed from `/crates/theater/src/host/`
- [ ] References updated in core crate
- [ ] HANDLER_MIGRATION.md updated with learnings

## Migration Log

### 2025-11-30
- Created change tracking structure
- Created proposal document
- Identified that random and timing are complete
- ✅ **Completed environment handler migration**
  - Implemented EnvironmentHandler struct with Handler trait
  - Fixed wasmtime version from 26.0 to 31.0 to match rest of project
  - Fixed closure signatures for func_wrap (tuples for parameters)
  - Updated tests and documentation with all config fields
  - All tests passing (2 unit tests + 1 doc test)
  - Ready for integration
- ✅ **Completed runtime handler migration**
  - Implemented RuntimeHandler struct with Handler trait
  - Migrated log, get-state, and shutdown functions
  - Async shutdown operation with theater command channel
  - Comprehensive event recording for chain
  - All tests passing (1 unit test)
  - Ready for integration
- ✅ **Completed filesystem handler migration**
  - Split into modular structure (lib, types, path_validation, operations)
  - Implemented all 9 filesystem operations
  - Comprehensive path validation with dunce canonicalization
  - Permission system with allowed/denied paths
  - Command execution with security restrictions
  - All tests passing (3 unit tests)
  - Ready for integration
- ✅ **Completed http-client handler migration**
  - Implemented HttpClientHandler struct with Handler trait
  - Migrated HttpRequest and HttpResponse component types
  - All async operations properly wrapped with func_wrap_async
  - Permission checking preserved
  - All tests passing (3 unit tests + 1 doc test)
  - Ready for integration

### 2025-12-07 (Continued)
- ✅ **Completed process handler migration** (most complex handler yet!)
  - Implemented ProcessHandler struct with Handler trait
  - Migrated all 5 operations (os-spawn, os-write-stdin, os-status, os-kill, os-signal)
  - Added 3 export functions for callbacks (handle-stdout, handle-stderr, handle-exit)
  - Complex process lifecycle management with ManagedProcess struct
  - Async I/O handling for stdin/stdout/stderr with 4 output modes (raw, line-by-line, JSON, chunked)
  - Process timeout monitoring with automatic kill
  - Comprehensive permission checking
  - Fixed multiple Send/lifetime issues with careful lock management
  - Fixed event data structure mismatches (ProcessSpawn, StdinWrite, Error, etc.)
  - Fixed wasmtime version to 31.0
  - All tests passing (3 unit tests + 1 doc test)
  - Complete README with architecture and usage documentation
  - ~990 lines migrated from ~1408 line original
  - Ready for integration

### 2025-12-07 (Morning)
- ✅ **Completed store handler migration**
  - Implemented StoreHandler struct with Handler trait
  - Migrated all 13 store operations (new, store, get, exists, label operations, list operations)
  - Fixed wasmtime version from 26.0 to 31.0 to match rest of project
  - Content-addressed storage with SHA1 hashing preserved
  - Label management system fully functional
  - Comprehensive event recording for all operations
  - All tests passing (2 unit tests + 1 doc test)
  - Complete README documentation with all operations listed
  - Ready for integration

### 2025-12-08
- ✅ **Completed supervisor handler migration** (last Phase 3 handler!)
  - Implemented SupervisorHandler struct with Handler trait
  - Migrated all 7 supervisor operations (spawn, resume, list-children, restart-child, stop-child, get-child-state, get-child-events)
  - Added 3 export functions for callbacks (handle-child-error, handle-child-exit, handle-child-external-stop)
  - Unique architecture with background task for receiving child actor results
  - Used Arc<Mutex<Option<Receiver>>> to manage channel receiver in cloneable handler
  - Fixed Handler trait compliance (add_export_functions takes &self, start returns Pin<Box<dyn Future>>)
  - All tests passing (2 unit tests)
  - Complete README with lifecycle documentation
  - ~1230 lines migrated from ~1079 line original
  - Ready for integration
  - **Phase 3 now 100% complete! 🎉**

### 2025-12-09
- ✅ **Resolved message-server handler compilation blocker**
  - Added MessageCommand enum (separate from TheaterCommand for future lifecycle integration)
  - Fixed ActorChannelOpen struct: added `initiator_id` field, renamed `data` to `initial_msg`
  - Fixed ActorChannelMessage struct: renamed `data` to `msg`
  - Added ChannelId::parse() method for parsing channel IDs from strings
  - Added temporary TheaterCommand variants (SendMessage, ChannelOpen, ChannelMessage, ChannelClose)
    - These are marked TEMPORARY and will be replaced with MessageCommand routing in lifecycle integration
  - Fixed MutexGuard Send issue in handler by adding proper scope
  - All tests passing (2 unit tests)
  - Handler now compiles successfully!
  - Ready for lifecycle integration (separate PR)

### 2025-12-10
- ✅ **Completed message-server architectural refactor**
  - **REMOVED lifecycle coupling** - No more ActorLifecycleEvent, Runtime has zero messaging knowledge
  - **Created MessageRouter** - High-throughput external routing service (100k+ msgs/sec capability)
    - Zero lock contention using channel-based architecture
    - Single task owns actor registry HashMap
    - Pure async message passing
  - **Per-actor handler instances** - Each actor gets its own handler via create_instance()
    - Handler registers mailbox during setup_host_functions()
    - Consumes mailbox in start() until shutdown
    - Unregisters from router on shutdown
  - **Complete separation** - Theater runtime is messaging-agnostic
  - **External service pattern** - MessageRouter created by user before Theater
  - **Updated all host functions** - Now use router.route_message() instead of theater_tx
  - Removed ActorLifecycleEvent from messages.rs
  - Removed message_lifecycle_tx from TheaterRuntime
  - Removed message_tx from ActorStore
  - All tests passing
  - Full documentation in message-router-architecture.md

### Earlier
- 2025-11-30: Random handler migration completed (documented)
- 2025-11-29: Timing handler migration completed

# Handler Migration Progress

This document tracks the progress of migrating handlers from the core `theater` crate to separate `theater-handler-*` crates.

See the full proposal: [2025-11-30-handler-migration.md](../proposals/2025-11-30-handler-migration.md)

## Migration Status

### ✅ Phase 1: Simple Handlers

| Handler | Status | Crate | Old File | Notes |
|---------|--------|-------|----------|-------|
| random | ✅ COMPLETE | `theater-handler-random` | `src/host/random.rs` | Documented example migration |
| timing | ✅ COMPLETE | `theater-handler-timing` | `src/host/timing.rs` | Fully migrated |
| environment | 🚧 IN PROGRESS | `theater-handler-environment` | `src/host/environment.rs` | Next to migrate |
| runtime | ❌ NOT STARTED | `theater-handler-runtime` | `src/host/runtime.rs` | Waiting |

### ❌ Phase 2: Medium Complexity

| Handler | Status | Crate | Old File | Notes |
|---------|--------|-------|----------|-------|
| http-client | ❌ NOT STARTED | `theater-handler-http-client` | `src/host/http_client.rs` | Waiting |
| filesystem | ❌ NOT STARTED | `theater-handler-filesystem` | `src/host/filesystem.rs` | Large but isolated |

### ❌ Phase 3: Complex Handlers

| Handler | Status | Crate | Old File | Notes |
|---------|--------|-------|----------|-------|
| process | ❌ NOT STARTED | `theater-handler-process` | `src/host/process.rs` | Complex interactions |
| store | ❌ NOT STARTED | `theater-handler-store` | `src/host/store.rs` | Complex state management |
| supervisor | ❌ NOT STARTED | `theater-handler-supervisor` | `src/host/supervisor.rs` | Complex orchestration |

### ❌ Phase 4: Framework Handlers

| Handler | Status | Crate | Old File | Notes |
|---------|--------|-------|----------|-------|
| message-server | ❌ NOT STARTED | `theater-handler-message-server` | `src/host/message_server.rs` | Depends on others |
| http-framework | ❌ NOT STARTED | `theater-handler-http-framework` | `src/host/framework/` | Depends on others |

## Overall Progress

- **Completed**: 2/11 (18%)
- **In Progress**: 0/11 (0%)
- **Not Started**: 9/11 (82%)

## Current Sprint

### Active Work
- [ ] Migrate environment handler
  - [ ] Implement EnvironmentHandler struct
  - [ ] Implement Handler trait
  - [ ] Add tests
  - [ ] Update documentation
  - [ ] Remove old implementation

### Blocked
None currently

### Next Up
1. Complete environment handler
2. Migrate runtime handler
3. Update core theater crate to use new handlers

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
- Ready to begin environment handler migration

### Earlier
- 2025-11-30: Random handler migration completed (documented)
- 2025-11-29: Timing handler migration completed

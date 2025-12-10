# Change Request: Handler Migration to Separate Crates

## Overview
Migrate all handler implementations from the core `theater` crate into separate `theater-handler-*` crates, following the pattern established by the `theater-handler-random` migration.

## Motivation
Currently, all handler implementations (environment, filesystem, http-client, process, etc.) are embedded within the core `theater` crate in `/crates/theater/src/host/`. This creates several issues:

1. **Tight coupling**: Handlers are tightly coupled to the core runtime, making them harder to maintain independently
2. **Difficult testing**: Testing handlers in isolation requires building the entire theater runtime
3. **Limited extensibility**: Third-party developers cannot easily create custom handlers following a clear pattern
4. **Complex dependencies**: All handler dependencies are bundled into the core crate, even if only a subset of handlers are used
5. **Harder to evolve**: Changes to individual handlers require rebuilding and testing the entire core crate

Moving handlers into separate crates provides:
- ✅ **Cleaner architecture** - Handlers are independent modules
- ✅ **Easier maintenance** - Each handler can evolve separately
- ✅ **Better testing** - Test handlers in isolation
- ✅ **Simpler lifetimes** - Synchronous trait methods avoid lifetime complexity
- ✅ **Third-party handlers** - Clear pattern for custom handlers
- ✅ **Modular dependencies** - Users can depend on only the handlers they need

## Detailed Design

### 1. Handler Trait Simplification
The core `Handler` trait has been simplified to make implementation easier:

**Before:**
```rust
fn setup_host_functions(
    &mut self,
    actor_component: &mut ActorComponent,
) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;
```

**After:**
```rust
fn setup_host_functions(
    &mut self,
    actor_component: &mut ActorComponent,
) -> Result<()>;
```

**Rationale:** None of the handlers actually used `.await` in their setup functions. Making them synchronous:
- Eliminated complex lifetime issues
- Made the code more honest about what it does
- Simplified implementation for all future handlers

### 2. Migration Pattern

Each handler migration follows this pattern:

#### Step 1: Create New Crate Structure
```
/crates/theater-handler-{name}/
  ├── Cargo.toml        # Dependencies and metadata
  ├── src/
  │   └── lib.rs        # Handler implementation
  └── README.md         # Documentation
```

#### Step 2: Copy and Adapt Implementation
1. Copy the host implementation from `/crates/theater/src/host/{name}.rs`
2. Rename `{Name}Host` → `{Name}Handler`
3. Update imports to use `theater::` prefix
4. Implement the `Handler` trait:
   - `create_instance()` - Clone yourself
   - `start()` - Async startup (keep as-is)
   - `setup_host_functions()` - Now synchronous!
   - `add_export_functions()` - Now synchronous!
   - `name()`, `imports()`, `exports()` - Metadata

#### Step 3: Update Dependencies
The handler crate depends on:
- Core theater crate (for trait definitions and types)
- Wasmtime (for WASM integration)
- Handler-specific dependencies
- Standard async/logging tools

#### Step 4: Remove Old Implementation
Once the new handler crate is tested and working:
1. Remove the old implementation from `/crates/theater/src/host/{name}.rs`
2. Update any references in the core crate to use the new handler crate
3. Update documentation

### 3. Handler List and Priority

#### ✅ Completed Migrations
1. **random** - COMPLETE (documented example)
2. **timing** - COMPLETE

#### 🚧 In Progress
None currently

#### ❌ Pending Migrations
Recommended order based on complexity:

**Phase 1: Simple Handlers**
3. **environment** - Provides env var access (simple, no complex state)
4. **runtime** - Runtime information (simple metadata)

**Phase 2: Medium Complexity**
5. **http-client** - HTTP requests (moderate complexity)
6. **filesystem** - File operations (larger but well-isolated)

**Phase 3: Complex Handlers**
7. **process** - OS process spawning (complex interactions)
8. **store** - Persistent storage (complex state management)
9. **supervisor** - Actor supervision (complex orchestration)

**Phase 4: Framework Handlers**
10. **message-server** - Inter-actor messaging (complex, depends on others)
11. **http-framework** - HTTP server framework (complex, depends on others)

### 4. Testing Strategy

For each migrated handler:
- ✅ Unit tests compile without errors
- ✅ Handler integrates with Theater runtime via `Handler` trait
- ✅ All existing functionality is preserved
- ✅ Chain events are logged correctly
- ✅ Permissions are enforced properly

### 5. Documentation Requirements

Each handler crate must include:
- Comprehensive rustdoc comments
- README.md with usage examples
- Migration notes if behavior changes
- Permission requirements

## Implementation Plan

### Phase 1: Foundation (Weeks 1-2)
- [x] Create top-level changes tracking structure
- [ ] Document migration pattern in detail
- [ ] Migrate environment handler
- [ ] Migrate runtime handler
- [ ] Update core theater crate to use new handlers

### Phase 2: Core Handlers (Weeks 3-4)
- [ ] Migrate http-client handler
- [ ] Migrate filesystem handler
- [ ] Update tests and documentation

### Phase 3: Complex Handlers (Weeks 5-7)
- [ ] Migrate process handler
- [ ] Migrate store handler
- [ ] Migrate supervisor handler

### Phase 4: Framework Handlers (Weeks 8-9)
- [ ] Migrate message-server handler
- [ ] Migrate http-framework handler
- [ ] Complete cleanup of old implementations

### Phase 5: Finalization (Week 10)
- [ ] Update all documentation
- [ ] Update examples
- [ ] Final testing
- [ ] Release notes

## Breaking Changes

This migration is designed to be non-breaking:
- The `Handler` trait is simplified but existing handlers can be easily adapted
- Old handler implementations remain until new ones are tested
- Users can gradually migrate to new handler crates
- The runtime behavior remains identical

However, there will be one breaking change:
- Dependencies on `theater` that use handlers directly will need to add dependencies on the specific `theater-handler-*` crates

## Migration Example: Random Handler

See `HANDLER_MIGRATION.md` in the root for the complete documented example of the random handler migration.

Key takeaways from the random handler migration:
- Trait simplification eliminated lifetime issues
- Clear separation of concerns
- All functionality preserved
- Better testability

## Success Criteria

The migration is complete when:
1. All 11 handlers are migrated to separate crates
2. All tests pass
3. Documentation is updated
4. Old implementations are removed from core crate
5. Examples are updated to use new handler crates
6. CI/CD pipeline passes
7. Performance benchmarks show no regression

## Alternatives Considered

### Alternative 1: Keep Handlers in Core
**Rejected:** Maintains tight coupling and makes it hard for third parties to create handlers

### Alternative 2: Dynamic Plugin System
**Rejected:** Adds complexity and runtime overhead. Static linking via Cargo is simpler and more efficient

### Alternative 3: Async Setup Functions
**Rejected:** None of the handlers need async setup, and it complicates lifetimes unnecessarily

## Impacts

### Positive
- Cleaner architecture
- Better modularity
- Easier to extend
- Clearer patterns for third-party handlers
- Simplified dependencies

### Negative
- More crates to maintain (mitigated by clear patterns)
- Slightly more complex dependency management for users
- Migration effort required

## Future Enhancements

After migration completion:
1. Consider versioning handlers independently
2. Create handler registry/marketplace
3. Add handler composition utilities
4. Develop handler testing framework
5. Create handler development guide

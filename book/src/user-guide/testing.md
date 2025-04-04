# Testing Strategy

This document outlines Theater's testing approach and infrastructure.

## Overview

Theater is a WebAssembly actor system with complex interactions between components including:
- Actor lifecycle management
- State verification through hash chains
- Message passing between actors
- Parent-child supervision relationships
- Various interface types (HTTP, messaging, etc.)
- Content-addressable storage

Given this architecture, our testing strategy focuses on both individual component reliability and system-wide interaction patterns. Theater uses a comprehensive testing strategy that combines unit tests, integration tests, and performance tests. Due to the highly interconnected nature of actor systems, we emphasize integration testing while maintaining good unit test coverage for individual components.

## Test Structure

```
tests/
├── common/                     # Shared testing utilities
│   ├── mod.rs                 # Common module definitions
│   ├── mocks.rs               # Common mocks (actors, handlers, etc.)
│   └── helpers.rs             # Test helper functions
├── integration/               # Full system tests
│   ├── lifecycle_tests.rs     # Actor lifecycle
│   ├── messaging_tests.rs     # Inter-actor communication
│   └── supervision_tests.rs   # Parent-child relationships
└── unit/                      # Unit tests for each component
    ├── runtime_tests.rs       # Theater/Actor runtime
    ├── handler_tests.rs       # Handler implementations
    └── store_tests.rs         # State storage
```

## WebAssembly Testing Strategy

Testing WebAssembly components requires special consideration:

### WASM Component Testing
- Using wasmtime for testing WASM modules
- Mocking host functions in tests
- Testing component interfaces
- Verifying WASM binary compatibility

### Test Environment
```rust
struct WasmTestContext {
    instance: wasmtime::Instance,
    store: wasmtime::Store<TestState>,
    // Additional test context...
}

impl WasmTestContext {
    async fn new() -> Result<Self> {
        // Setup WASM test environment
    }
}
```

### Interface Testing
- Validating interface implementations
- Testing interface versioning
- Checking component linking
- Verifying capability access

## Failure Testing

Theater must be resilient to various failure scenarios:

### Actor Failures
- Crash recovery
- State recovery
- Resource cleanup
- Error propagation

### System Failures
- Network partitions
- Resource exhaustion
- Partial system failure
- Recovery procedures

### Example Failure Test
```rust
#[tokio::test]
async fn test_actor_crash_recovery() {
    let ctx = TestContext::new().await?;
    
    // Force actor crash
    ctx.simulate_actor_crash(actor_id).await?;
    
    // Verify recovery
    assert_eq!(ctx.get_actor_status(actor_id).await?, ActorStatus::Recovered);
}
```

## Distributed Testing

Testing distributed aspects of the system:

### Network Scenarios
- Message ordering
- Network partitions
- Node failures
- State synchronization

### Consistency Testing
- Event ordering verification
- State replication
- Consensus verification
- Partition tolerance

## Testing Tools & Infrastructure

### Custom Test Helpers
```rust
/// Simulates network conditions
pub struct NetworkSimulator {
    pub latency: Duration,
    pub packet_loss: f64,
}

/// Manages test actor system
pub struct ActorTestHarness {
    pub runtime: TheaterRuntime,
    pub network: NetworkSimulator,
}
```

### CI/CD Integration
- Test matrix configuration
- Performance benchmarks
- Integration test suites
- Coverage reporting

## Storage Testing

Testing the content-addressable storage system:

### State Persistence
- Verifying state saves/loads
- Testing concurrent access
- Checking data integrity
- Testing storage limits

### Hash Chain Verification
```rust
#[tokio::test]
async fn test_hash_chain_integrity() {
    let store = TestStore::new().await?;
    
    // Add events
    store.append_event(test_event).await?;
    
    // Verify chain
    assert!(store.verify_chain().await?);
}
```

## Metrics & Observability

### Logging in Tests
```rust
#[test_log::test(tokio)]
async fn test_with_logs() {
    tracing::info!("Starting test");
    // Test implementation
}
```

### Metrics Collection
- Performance metrics
- Resource usage
- Timing measurements
- System health indicators

### Tracing
- Distributed tracing in tests
- Span verification
- Causal ordering checks

## Testing Patterns

### Actor Testing Patterns
1. Spawn-Test-Stop Pattern
```rust
async fn test_actor_pattern<T: TestActor>(actor: T) {
    // Setup
    let id = spawn_actor(actor).await?;
    
    // Test
    send_test_messages(id).await?;
    verify_state(id).await?;
    
    // Cleanup
    stop_actor(id).await?;
}
```

2. Supervision Pattern
```rust
async fn test_supervision_pattern() {
    let parent = spawn_parent().await?;
    let child = spawn_child(parent).await?;
    
    simulate_child_failure(child).await?;
    verify_parent_handles_failure(parent).await?;
}
```

### Async Testing
- Handling timeouts
- Race condition testing
- Concurrent operation testing
- Event ordering verification

## Version & Compatibility Testing

### Version Testing
- Testing version upgrades
- State migration tests
- Protocol compatibility
- Interface versioning

### Compatibility Matrix
```rust
#[tokio::test]
async fn test_version_compatibility() {
    for old_version in supported_versions {
        // Test upgrade scenario
        test_upgrade_from_version(old_version).await?;
        
        // Test downgrade scenario
        test_downgrade_to_version(old_version).await?;
    }
}
```

## Running Tests

```bash
# Run all tests
cargo test

# Run specific test categories
cargo test --test integration
cargo test --test unit

# Run with logging
RUST_LOG=debug cargo test

# Run with test coverage
cargo tarpaulin
```

## Adding New Tests

When adding new functionality:

1. Add unit tests for individual components
2. Add integration tests for component interactions
3. Update mocks if needed
4. Consider performance implications

## Test Environment

Tests should:
- Use temporary directories for file operations
- Mock external services
- Clean up resources after completion
- Be independent and repeatable

## Best Practices

1. Test Coverage
   - All new code should include tests
   - Critical paths should have integration tests
   - Edge cases should be explicitly tested

2. Test Organization
   - Tests should be organized by functionality
   - Helper functions should be shared via common modules
   - Test names should clearly describe their purpose

3. Async Testing
   - Use appropriate async test helpers
   - Handle timeouts properly
   - Test both success and failure paths

4. State Management
   - Clean up test state after each test
   - Use temporary resources where appropriate
   - Isolate test state between runs

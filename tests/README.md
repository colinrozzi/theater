# Theater Testing Suite

This directory contains tests for the Theater WebAssembly actor system. The test suite is organized into three main sections:

## Directory Structure

```
tests/
├── common/                     # Shared testing utilities
│   ├── mod.rs                 # Common module definitions
│   ├── mocks.rs               # Common mocks (actors, handlers, etc.)
│   └── helpers.rs             # Test helper functions
├── integration/               # Full system tests
│   ├── lifecycle_tests.rs     # Actor lifecycle
│   └── messaging_tests.rs     # Inter-actor communication
└── unit/                      # Unit tests for each component
    ├── chain_tests.rs         # StateChain component tests
    ├── actor_store_tests.rs   # ActorStore component tests
    ├── actor_handle_tests.rs  # ActorHandle component tests
    ├── messages_tests.rs      # Message handling tests
    └── store_tests.rs         # Content store tests
```

## Running Tests

You can run tests using Cargo:

```bash
# Run all tests
cargo test

# Run only unit tests
cargo test --test unit

# Run only integration tests
cargo test --test integration

# Run a specific test
cargo test --test unit::chain_tests

# Run tests with logs
RUST_LOG=debug cargo test -- --nocapture
```

## Test Categories

1. **Unit Tests**: These test individual components in isolation.
   - StateChain tests verify the integrity and functionality of the event chain
   - ActorStore tests cover state management and event recording
   - ActorHandle tests verify communication with actors
   - Messages tests ensure correct serialization and routing

2. **Integration Tests**: These test how components work together.
   - Lifecycle tests verify actor creation, initialization, and shutdown
   - Messaging tests verify inter-actor communication

## Creating New Tests

When adding a new feature or component:

1. Add unit tests for your component in `tests/unit/`
2. Add integration tests for component interactions in `tests/integration/`
3. Add any necessary mocks to `tests/common/mocks.rs`
4. Add any helper functions to `tests/common/helpers.rs`

## Testing Patterns

- Use `tokio::test` for async tests
- Use `test_log::test(tokio)` for tests with logging
- Use tempfile for filesystem operations
- Clean up resources after test completion
- Use mocks for external dependencies

## Best Practices

- Test both success and error paths
- Add comments explaining test purpose
- Keep tests independent and repeatable
- Use descriptive test names
- Prefer table-driven tests for related test cases

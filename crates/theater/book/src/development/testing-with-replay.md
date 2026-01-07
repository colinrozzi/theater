# Testing Actors with Deterministic Replay

Theater's replay system allows you to verify that your actors behave deterministically. By recording an actor's execution and then replaying it, you can ensure that given the same inputs, your actor produces the exact same outputs - a critical property for debugging, auditing, and building trust in your actor systems.

## Overview

### What is Replay Verification?

Replay verification works in two phases:

1. **Recording Phase**: Run your actor normally and capture all events (host function calls, WASM executions) into an event chain
2. **Replay Phase**: Run the same actor with a special handler that returns the recorded outputs instead of making real calls
3. **Verification**: Compare the event chain hashes - if they match, your actor is deterministic

### Why Use Replay Testing?

- **Verify Determinism**: Ensure your actor produces identical behavior given the same inputs
- **Debugging**: Reproduce exact execution sequences to diagnose issues
- **Regression Testing**: Verify that code changes don't affect deterministic behavior
- **Audit Trail**: Prove that an actor behaved correctly at a specific point in time

## Quick Start

### Prerequisites

You need a compiled WASM actor component. This example uses the `runtime-test` actor:

```bash
# Build the test actor
cd crates/theater-handler-runtime/test-actors/runtime-test
cargo component build --release
```

### Run the Replay Test

```bash
# Run the replay verification test
cargo test -p theater-replay-experimenting -- --nocapture
```

Expected output:
```
running 2 tests
test tests::test_replay_verification ... ok
test tests::test_replay_event_types ... ok

test result: ok. 2 passed; 0 failed; 0 ignored
```

### Run the Interactive Experiment

For more detailed output:

```bash
cargo run -p theater-replay-experimenting
```

This shows:
- Recording phase with event collection
- Replay phase with the replay handler
- Side-by-side hash comparison
- Verification summary

## How It Works

### Event Chain Structure

Every host function call is recorded as a `ChainEvent`:

```rust
pub struct ChainEvent {
    pub hash: Vec<u8>,              // SHA1 hash of this event
    pub parent_hash: Option<Vec<u8>>, // Links to previous event
    pub event_type: String,          // e.g., "theater:simple/runtime/log"
    pub data: Vec<u8>,               // Serialized HostFunctionCall
}
```

The `data` field contains a serialized `HostFunctionCall`:

```rust
pub struct HostFunctionCall {
    pub interface: String,  // e.g., "theater:simple/runtime"
    pub function: String,   // e.g., "log"
    pub input: Vec<u8>,     // Serialized input parameters
    pub output: Vec<u8>,    // Serialized return value
}
```

### Hash Chaining

Each event's hash is computed from:
- The event type
- The event data (serialized HostFunctionCall)
- The parent hash (if any)

This creates a tamper-evident chain - modifying any event breaks all subsequent hashes.

### The Replay Handler

During replay, the `ReplayHandler`:

1. Intercepts all host function calls
2. Finds the matching recorded event
3. Returns the recorded output (instead of making real calls)
4. Records a new event with the actual inputs and recorded outputs
5. The new chain should have identical hashes if the actor is deterministic

## Writing Replay Tests

### Basic Test Structure

```rust
use theater_replay_experimenting::run_replay_verification;

#[tokio::test]
async fn test_my_actor_is_deterministic() {
    let chain_path = format!("/tmp/test_chain_{}.json", std::process::id());

    let result = run_replay_verification(&chain_path, false)
        .await
        .expect("Replay verification should complete");

    // Clean up
    let _ = std::fs::remove_file(&chain_path);

    // Verify determinism
    assert!(result.passed, "Actor should be deterministic");
    assert_eq!(result.mismatches, 0, "All hashes should match");
}
```

### Understanding Results

The `ReplayVerificationResult` provides:

```rust
pub struct ReplayVerificationResult {
    pub original_chain: Vec<ChainEvent>,  // Events from recording
    pub replay_chain: Vec<ChainEvent>,    // Events from replay
    pub mismatches: usize,                 // Number of hash differences
    pub passed: bool,                      // Overall verification status
}
```

### Testing Specific Event Types

```rust
#[tokio::test]
async fn test_actor_logs_correctly() {
    let result = run_replay_verification(&chain_path, false).await?;

    // Check for specific event types
    let log_events: Vec<_> = result.original_chain
        .iter()
        .filter(|e| e.event_type.contains("runtime/log"))
        .collect();

    assert!(!log_events.is_empty(), "Actor should produce log events");
}
```

## Setting Up Your Actor for Replay

### Manifest Configuration

Your actor manifest needs the `runtime` handler at minimum:

```toml
name = "my-actor"
version = "0.1.0"
component = "target/wasm32-unknown-unknown/release/my_actor.wasm"
save_chain = true  # Important for saving the event chain

[[handler]]
type = "runtime"
```

### For Replay Mode

Add a replay handler configuration pointing to the recorded chain:

```toml
name = "my-actor-replay"
version = "0.1.0"
component = "target/wasm32-unknown-unknown/release/my_actor.wasm"
save_chain = true

[[handler]]
type = "replay"
chain = "/path/to/recorded_chain.json"

[[handler]]
type = "runtime"
```

## Common Sources of Non-Determinism

### 1. Timestamps

**Problem**: Using current time directly
```rust
// Non-deterministic!
let now = std::time::SystemTime::now();
```

**Solution**: Use the timing handler's recorded values
```rust
// Deterministic - time is recorded and replayed
let now = timing::now();
```

### 2. Random Numbers

**Problem**: Using system random
```rust
// Non-deterministic!
let value = rand::random::<u64>();
```

**Solution**: Use the random handler
```rust
// Deterministic - random values are recorded and replayed
let value = random::get_random_u64();
```

### 3. External API Calls

**Problem**: Making HTTP calls that return different data
```rust
// Non-deterministic if API returns different data!
let response = http::get("https://api.example.com/data");
```

**Solution**: Use recorded responses during replay - the replay handler automatically returns the recorded response.

### 4. Actor IDs

**Problem**: Using actor IDs in computation
```rust
// Actor IDs differ between runs!
let key = format!("data-{}", actor_id);
```

**Solution**: Use stable identifiers that don't depend on actor ID.

## Troubleshooting

### Hash Mismatch Errors

If you see hash mismatches:

1. **Check event order**: Print both chains to see where they diverge
```rust
if !result.passed {
    println!("{}", result.comparison_details());
}
```

2. **Look for non-deterministic operations**: Timestamps, random numbers, etc.

3. **Verify inputs are identical**: The replay uses recorded outputs but actual inputs - if inputs differ, hashes will differ.

### Chain Length Differences

If chain lengths differ:
- The actor may be making different numbers of calls
- Some calls may be conditional on non-deterministic values
- Error handling may differ between runs

### Missing Events

If events are missing during replay:
- The replay handler may not be intercepting all interfaces
- Check that all required handlers are configured

## Advanced Usage

### Custom Verification Logic

```rust
async fn verify_with_custom_checks(chain_path: &str) -> Result<()> {
    let result = run_replay_verification(chain_path, false).await?;

    // Custom verification
    for (i, (orig, replay)) in result.original_chain
        .iter()
        .zip(result.replay_chain.iter())
        .enumerate()
    {
        if orig.hash != replay.hash {
            // Detailed analysis
            let orig_data: HostFunctionCall = serde_json::from_slice(&orig.data)?;
            let replay_data: HostFunctionCall = serde_json::from_slice(&replay.data)?;

            println!("Mismatch at event {}:", i);
            println!("  Original input:  {:?}", orig_data.input);
            println!("  Replay input:    {:?}", replay_data.input);
        }
    }

    Ok(())
}
```

### Integration with CI/CD

Add replay tests to your CI pipeline:

```yaml
# .github/workflows/test.yml
test:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v2

    - name: Build test actors
      run: |
        cd test-actors/my-actor
        cargo component build --release

    - name: Run replay tests
      run: cargo test -p theater-replay-experimenting
```

## API Reference

### `run_replay_verification`

```rust
pub async fn run_replay_verification(
    chain_path: &str,  // Path to save/load the chain file
    verbose: bool,     // Print progress information
) -> Result<ReplayVerificationResult>
```

### `ReplayVerificationResult`

```rust
impl ReplayVerificationResult {
    /// Check if chains have the same length
    pub fn same_length(&self) -> bool;

    /// Get detailed comparison for debugging
    pub fn comparison_details(&self) -> String;
}
```

### `ReplayHandler`

```rust
impl ReplayHandler {
    /// Create a new replay handler from recorded events
    pub fn new(expected_chain: Vec<ChainEvent>) -> Self;

    /// Get current replay progress
    pub fn progress(&self) -> (usize, usize);
}
```

## Best Practices

1. **Test Early**: Add replay tests as soon as you have a working actor
2. **Test Often**: Run replay tests on every commit
3. **Isolate Non-Determinism**: Keep non-deterministic operations in handlers
4. **Document Assumptions**: Note any dependencies on external state
5. **Version Your Chains**: Keep recorded chains with your test fixtures
6. **Use Unique Temp Files**: Avoid test collisions with unique paths

## Next Steps

- Learn about [Building Actors](./building-actors.md) for actor development
- Explore [Event Chains](./concepts/event-chain.md) for chain internals
- See [Traceability](../core-concepts/traceability.md) for the conceptual overview

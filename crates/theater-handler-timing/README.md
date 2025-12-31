# theater-handler-timing

WASI clocks handler for Theater providing monotonic and wall clock interfaces, along with poll-based timing subscriptions.

## Overview

This handler implements the WASI clocks interfaces that enable WebAssembly components to work with time - reading the current time, measuring durations, and creating pollable timers for async operations.

## Interfaces Provided

### WASI Clocks (wasi:clocks@0.2.3)

- **wasi:clocks/wall-clock** - Wall clock time (system time that can be affected by NTP adjustments)
  - `now()` - Get current wall clock time
  - `resolution()` - Get clock resolution

- **wasi:clocks/monotonic-clock** - Monotonic clock for measuring elapsed time
  - `now()` - Get current monotonic instant (nanoseconds)
  - `resolution()` - Get clock resolution  
  - `subscribe-instant(instant)` - Create a pollable that becomes ready at a specific instant
  - `subscribe-duration(duration)` - Create a pollable that becomes ready after a duration

### WASI I/O Poll (wasi:io/poll@0.2.3)

- **wasi:io/poll** - Pollable resources for async I/O
  - `pollable.ready()` - Check if pollable is ready
  - `pollable.block()` - Block until pollable becomes ready
  - `poll(pollables)` - Wait for any pollable to become ready

## Configuration

In actor manifests:

```toml
[[handler]]
type = "timing"
max_sleep_duration = 3600000  # Maximum sleep duration in milliseconds
min_sleep_duration = 1        # Minimum sleep duration in milliseconds
```

## Architecture

### Pollable Implementation

The handler creates pollable resources backed by Tokio's async timers:
- `subscribe-instant` creates a timer for an absolute timestamp
- `subscribe-duration` creates a timer for a relative duration
- `poll()` uses `tokio::select!` to wait on multiple timers efficiently

### Event Recording

The handler records timing events for audit purposes:
- Clock resolution queries
- Time reads (wall clock and monotonic)
- Timer subscriptions
- Poll operations with ready counts

## Usage

### In Test Actors

Create a WIT world that imports the interfaces:

```wit
package my:actor;

world my-actor {
    import wasi:clocks/wall-clock@0.2.3;
    import wasi:clocks/monotonic-clock@0.2.3;
    import wasi:io/poll@0.2.3;
    export theater:simple/actor;
}
```

### Rust Actor Example

```rust
use bindings::wasi::clocks::monotonic_clock;
use bindings::wasi::io::poll;

fn example() {
    // Get current time
    let now = monotonic_clock::now();
    
    // Create a timer for 100ms from now
    let timer = monotonic_clock::subscribe_duration(100_000_000);
    
    // Block until timer fires
    timer.block();
    
    // Or poll multiple timers
    let timer1 = monotonic_clock::subscribe_duration(50_000_000);
    let timer2 = monotonic_clock::subscribe_duration(100_000_000);
    let ready = poll::poll(&[&timer1, &timer2]);
    // ready contains indices of fired timers
}
```

### Direct Handler Usage

When building custom handler registries:

```rust
use theater_handler_timing::TimingHandler;
use theater::config::actor_manifest::TimingHostConfig;
use theater::handler::HandlerRegistry;

let config = TimingHostConfig {
    max_sleep_duration: 3600000,
    min_sleep_duration: 1,
};

let mut registry = HandlerRegistry::new();
registry.register(TimingHandler::new(config, None));
```

## Dependencies

This handler typically works alongside:
- `theater-handler-io` - Provides base `wasi:io/poll` interface (timing adds pollable implementations)

Note: The timing handler provides its own `wasi:io/poll` implementation. If both timing and IO handlers are active, the timing handler's poll interface is used when timing interfaces are imported.

## Testing

### Build Test Actor

```bash
cd test-actors/wasi-clocks-test
cargo component build --release
```

### Run Integration Tests

```bash
cargo test --test integration_test -- --nocapture
```

## Module Structure

- `lib.rs` - Handler implementation and trait impls
- `host_impl.rs` - Host trait implementations for clock interfaces
- `events.rs` - Event data types for audit trail
- `bindings.rs` - Generated wasmtime bindings

## Events

The handler emits `TimingEventData` events:

```rust
pub enum TimingEventData {
    HandlerSetupStart,
    HandlerSetupSuccess,
    LinkerInstanceSuccess,
    WallClockNow { seconds: u64, nanoseconds: u32 },
    MonotonicClockNow { instant: u64 },
    MonotonicClockResolution { resolution: u64 },
    SubscribeInstant { when: u64 },
    SubscribeDuration { duration: u64 },
    PollReady { ready_count: usize },
}
```

## Security Considerations

- `max_sleep_duration` prevents actors from sleeping indefinitely
- `min_sleep_duration` prevents tight-loop polling
- All timing operations are recorded in the event chain

## License

MIT OR Apache-2.0

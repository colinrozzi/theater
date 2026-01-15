# Timing Handler

The Timing Handler provides actors with WASI-compliant timing capabilities, including wall clock time, monotonic time for measuring durations, and polling for async operations.

## Overview

The Timing Handler implements the standard WASI clocks interfaces:

- `wasi:clocks/wall-clock@0.2.3` - Real-world wall clock time
- `wasi:clocks/monotonic-clock@0.2.3` - Monotonic time for measuring durations
- `wasi:io/poll@0.2.3` - Polling interface for async operations

## Configuration

To use the Timing Handler, add it to your actor's manifest:

```toml
[[handlers]]
type = "timing"
```

## WASI Clocks Interfaces

### Wall Clock

The wall clock provides real-world time that can be displayed to users.

```wit
interface wall-clock {
    /// A time and date in seconds plus nanoseconds.
    record datetime {
        seconds: u64,
        nanoseconds: u32,
    }

    /// Read the current value of the clock.
    now: func() -> datetime;

    /// Query the resolution of the clock.
    resolution: func() -> datetime;
}
```

#### Usage Example

```rust
use wasi::clocks::wall_clock;

// Get current wall clock time
let now = wall_clock::now();
println!("Current time: {} seconds, {} nanoseconds", now.seconds, now.nanoseconds);

// Get clock resolution
let resolution = wall_clock::resolution();
println!("Clock resolution: {} ns", resolution.nanoseconds);
```

### Monotonic Clock

The monotonic clock provides time that only moves forward, useful for measuring durations.

```wit
interface monotonic-clock {
    /// An instant in time, in nanoseconds.
    type instant = u64;

    /// A duration of time, in nanoseconds.
    type duration = u64;

    /// Read the current value of the clock.
    now: func() -> instant;

    /// Query the resolution of the clock.
    resolution: func() -> duration;

    /// Create a pollable that becomes ready when the given instant is reached.
    subscribe-instant: func(when: instant) -> pollable;

    /// Create a pollable that becomes ready after the given duration.
    subscribe-duration: func(when: duration) -> pollable;
}
```

#### Usage Example

```rust
use wasi::clocks::monotonic_clock;
use wasi::io::poll;

// Measure elapsed time
let start = monotonic_clock::now();

// ... perform operation ...

let end = monotonic_clock::now();
let elapsed_ns = end - start;
let elapsed_ms = elapsed_ns / 1_000_000;
println!("Operation took {} ms", elapsed_ms);

// Sleep for a duration using pollables
let duration_ns = 1_000_000_000; // 1 second
let pollable = monotonic_clock::subscribe_duration(duration_ns);
poll::poll(&[&pollable]);
println!("Waited 1 second");
```

### Polling

The poll interface provides async waiting capabilities.

```wit
interface poll {
    /// A pollable handle.
    resource pollable {
        /// Return true if the pollable is ready.
        ready: func() -> bool;

        /// Block until the pollable is ready.
        block: func();
    }

    /// Poll for ready events on multiple pollables.
    poll: func(in: list<borrow<pollable>>) -> list<u32>;
}
```

#### Usage Example

```rust
use wasi::clocks::monotonic_clock;
use wasi::io::poll;

// Create multiple pollables
let p1 = monotonic_clock::subscribe_duration(1_000_000_000); // 1 second
let p2 = monotonic_clock::subscribe_duration(2_000_000_000); // 2 seconds

// Poll for any to become ready
let ready_indices = poll::poll(&[&p1, &p2]);
println!("Ready pollables: {:?}", ready_indices);
```

## Replay Support

All timing operations are recorded in the actor's event chain with full type information for deterministic replay:

- Wall clock `now()` calls record the returned datetime
- Monotonic clock calls record instants and pollable IDs
- Poll operations record which pollables became ready

During replay, the handler returns the recorded values instead of querying system time, ensuring deterministic behavior.

## Common Patterns

### Implementing Timeouts

```rust
use wasi::clocks::monotonic_clock;
use wasi::io::poll;

fn with_timeout<F, T>(timeout_ns: u64, operation: F) -> Option<T>
where
    F: FnOnce() -> Option<T>,
{
    // Create a timeout pollable
    let timeout = monotonic_clock::subscribe_duration(timeout_ns);

    // Check if timeout has elapsed
    if timeout.ready() {
        return None;
    }

    // Perform operation
    operation()
}
```

### Measuring Performance

```rust
use wasi::clocks::monotonic_clock;

fn measure<F, T>(operation: F) -> (T, u64)
where
    F: FnOnce() -> T,
{
    let start = monotonic_clock::now();
    let result = operation();
    let end = monotonic_clock::now();
    (result, end - start)
}

// Usage
let (result, duration_ns) = measure(|| expensive_computation());
println!("Computation took {} ms", duration_ns / 1_000_000);
```

### Periodic Tasks

```rust
use wasi::clocks::monotonic_clock;
use wasi::io::poll;

fn run_every(interval_ns: u64, mut task: impl FnMut()) {
    loop {
        task();

        let pollable = monotonic_clock::subscribe_duration(interval_ns);
        pollable.block();
    }
}

// Run every second
run_every(1_000_000_000, || {
    println!("Tick!");
});
```

## Security Considerations

1. **Timing Side Channels**: Be aware that timing information can leak sensitive data
2. **Resource Usage**: Creating many pollables consumes resources
3. **Replay Consistency**: In replay mode, recorded times are used instead of real time

## Related Topics

- [Runtime Handler](runtime.md) - For runtime information and operations
- [WASI Specification](https://github.com/WebAssembly/wasi-clocks) - Official WASI clocks specification

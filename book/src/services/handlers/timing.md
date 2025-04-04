# Timing Handler

The Timing Handler provides actors with time-related capabilities, including delays, periodic scheduling, and timeout management. It enables actors to control the timing of their operations while maintaining Theater's state verification model.

## Overview

The Timing Handler implements the `ntwk:theater/timing` interface, enabling actors to:

1. Introduce controlled delays in their execution
2. Implement timeout patterns for operations
3. Enforce rate limits and throttling
4. Create periodic tasks and scheduled operations

## Configuration

To use the Timing Handler, add it to your actor's manifest:

```toml
[[handlers]]
type = "timing"
config = { 
    max_sleep_duration = 3600000,  # Maximum sleep duration in milliseconds (1 hour)
    min_sleep_duration = 1         # Minimum sleep duration in milliseconds
}
```

Configuration options:

* `max_sleep_duration`: (Optional) Maximum allowed sleep duration in milliseconds, defaults to 3600000 (1 hour)
* `min_sleep_duration`: (Optional) Minimum allowed sleep duration in milliseconds, defaults to 1

## Interface

The Timing Handler is defined using the following WIT interface:

```wit
interface timing {
    // Sleep for the specified duration (in milliseconds)
    sleep: func(duration-ms: u64) -> result<_, string>;
    
    // Get current timestamp (milliseconds since epoch)
    now: func() -> u64;
    
    // Get high-resolution time for performance measurement (in nanoseconds)
    performance-now: func() -> u64;
}
```

## Basic Timing Operations

### Sleep

The `sleep` function pauses actor execution for a specified duration:

```rust
// Sleep for 1 second
match timing::sleep(1000) {
    Ok(_) => {
        println!("Resumed after 1 second");
    },
    Err(error) => {
        println!("Sleep operation failed: {}", error);
    }
}
```

Note that the sleep duration must fall within the configured `min_sleep_duration` and `max_sleep_duration` range. Attempting to sleep for longer than the maximum or shorter than the minimum will result in an error.

### Current Time

The `now` function returns the current time in milliseconds since the Unix epoch:

```rust
let current_time = timing::now();
println!("Current time: {} ms", current_time);
```

### Performance Timing

The `performance-now` function provides high-resolution timing for performance measurement:

```rust
// Measure operation duration
let start = timing::performance_now();

// Perform operation
perform_expensive_operation();

let end = timing::performance_now();
let duration_ns = end - start;
let duration_ms = duration_ns / 1_000_000;

println!("Operation took {} ms", duration_ms);
```

## Common Patterns

### Implementing Timeouts

```rust
// Perform an operation with a timeout
fn perform_with_timeout<F, T>(operation: F, timeout_ms: u64) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String>,
{
    // Create a oneshot channel for the result
    let (tx, rx) = tokio::sync::oneshot::channel();
    
    // Spawn a task to perform the operation
    tokio::spawn(async move {
        match operation() {
            Ok(result) => {
                let _ = tx.send(Ok(result));
            },
            Err(err) => {
                let _ = tx.send(Err(err));
            }
        }
    });
    
    // Wait for the result or timeout
    match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), rx).await {
        Ok(result) => result.unwrap(),
        Err(_) => Err("Operation timed out".to_string()),
    }
}

// Usage
let result = perform_with_timeout(|| {
    // Perform potentially long-running operation
    perform_api_call()
}, 5000); // 5 second timeout
```

### Rate Limiting

```rust
// Simple rate limiter
struct RateLimiter {
    last_operation: u64,
    min_interval_ms: u64,
}

impl RateLimiter {
    fn new(min_interval_ms: u64) -> Self {
        Self {
            last_operation: 0,
            min_interval_ms,
        }
    }
    
    fn check_and_update(&mut self) -> Result<(), String> {
        let now = timing::now();
        let elapsed = now - self.last_operation;
        
        if self.last_operation == 0 || elapsed >= self.min_interval_ms {
            self.last_operation = now;
            Ok(())
        } else {
            let wait_time = self.min_interval_ms - elapsed;
            timing::sleep(wait_time)?;
            self.last_operation = timing::now();
            Ok(())
        }
    }
}

// Usage
let mut rate_limiter = RateLimiter::new(100); // 100ms between operations

for item in items {
    // Ensure we don't exceed rate limit
    rate_limiter.check_and_update()?;
    
    // Process item
    process_item(item)?;
}
```

### Periodic Tasks

```rust
// Run a task periodically
fn run_periodically<F>(task: F, interval_ms: u64, max_iterations: Option<usize>) -> Result<(), String>
where
    F: Fn() -> Result<(), String>,
{
    let mut iterations = 0;
    
    loop {
        // Run the task
        task()?;
        
        // Check if we've reached the maximum iterations
        if let Some(max) = max_iterations {
            iterations += 1;
            if iterations >= max {
                break;
            }
        }
        
        // Sleep until the next interval
        timing::sleep(interval_ms)?;
    }
    
    Ok(())
}

// Usage
run_periodically(|| {
    // Periodic task logic
    collect_metrics()
}, 5000, Some(10))?; // Run every 5 seconds, 10 times
```

## State Chain Integration

All timing operations are recorded in the actor's state chain, creating a verifiable history. The chain events include:

1. **TimingOperation**: Records details of timing operations:
   - Operation type (sleep, now, performance-now)
   - Duration (for sleep operations)
   - Timestamp

2. **Error**: Records any errors that occur:
   - Operation type
   - Error message

This integration ensures that all timing activities are:
- Traceable
- Verifiable
- Reproducible
- Auditable

## Error Handling

The Timing Handler provides error information for various failure scenarios:

1. **Duration Errors**: When sleep duration is outside allowed range
2. **Operation Errors**: When timing operations fail
3. **Resource Errors**: When system resources are unavailable

## Security Considerations

When using the Timing Handler, consider the following security aspects:

1. **Sleep Limits**: The configuration enforces limits on sleep durations
2. **Resource Consumption**: Long or frequent sleeps may impact system resources
3. **Timing Attacks**: Be aware of potential timing side-channel attacks

## Implementation Details

Under the hood, the Timing Handler:

1. Uses the Tokio runtime for asynchronous sleep operations
2. Enforces configurable minimum and maximum sleep durations
3. Records all operations in the state chain
4. Provides consistent time sources across the actor system

## Performance Considerations

1. **Sleep Overhead**: There is a small overhead for each sleep operation
2. **Time Resolution**: Time functions have platform-dependent resolution
3. **Resource Usage**: Excessive sleep operations may impact system performance

## Best Practices

1. **Error Handling**: Always handle errors from timing functions
2. **Sleep Duration**: Use reasonable sleep durations
3. **Batch Processing**: Consider batching operations instead of sleeping between each
4. **Timeouts**: Implement timeouts for operations that may not complete
5. **Rate Limiting**: Use rate limiting for external API calls

## Examples

### Retry Logic

```rust
// Retry an operation with exponential backoff
fn retry_with_backoff<F, T>(
    operation: F,
    initial_backoff_ms: u64,
    max_backoff_ms: u64,
    max_retries: usize
) -> Result<T, String>
where
    F: Fn() -> Result<T, String>,
{
    let mut backoff = initial_backoff_ms;
    let mut attempts = 0;
    
    loop {
        match operation() {
            Ok(result) => return Ok(result),
            Err(error) => {
                attempts += 1;
                
                if attempts >= max_retries {
                    return Err(format!("Operation failed after {} attempts: {}", attempts, error));
                }
                
                // Log the failure and retry plan
                println!("Attempt {} failed: {}. Retrying in {} ms", attempts, error, backoff);
                
                // Wait before next attempt
                timing::sleep(backoff)?;
                
                // Exponential backoff with jitter
                let jitter = (backoff as f64 * 0.1 * rand::random::<f64>()) as u64;
                backoff = std::cmp::min(backoff * 2 + jitter, max_backoff_ms);
            }
        }
    }
}

// Usage
let result = retry_with_backoff(
    || external_api_call("https://api.example.com/data"),
    100,    // Initial backoff of 100ms
    30000,  // Maximum backoff of 30 seconds
    5       // Maximum 5 retry attempts
)?;
```

### Debouncing

```rust
// Debounce a function call
struct Debouncer {
    last_call: u64,
    timeout_ms: u64,
}

impl Debouncer {
    fn new(timeout_ms: u64) -> Self {
        Self {
            last_call: 0,
            timeout_ms,
        }
    }
    
    fn should_call(&mut self) -> bool {
        let now = timing::now();
        
        if now - self.last_call >= self.timeout_ms {
            self.last_call = now;
            true
        } else {
            false
        }
    }
}

// Usage
let mut input_debouncer = Debouncer::new(500); // 500ms debounce

fn process_input(input: &str) {
    if input_debouncer.should_call() {
        // Process the input
        println!("Processing input: {}", input);
    } else {
        // Skip processing this input
        println!("Debounced input: {}", input);
    }
}
```

### Measuring Request Latency

```rust
// Measure and log request latency
fn measure_request_latency<F, T>(operation_name: &str, operation: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String>,
{
    // Get start time in high resolution
    let start = timing::performance_now();
    
    // Perform the operation
    let result = operation()?;
    
    // Calculate duration
    let end = timing::performance_now();
    let duration_ns = end - start;
    let duration_ms = duration_ns / 1_000_000;
    
    // Log the latency
    println!("{} took {} ms", operation_name, duration_ms);
    
    // Record metric
    runtime::record_metric(&format!("{}_latency_ms", operation_name), duration_ms as f64)?;
    
    // Return the result
    Ok(result)
}

// Usage
let user_data = measure_request_latency("fetch_user_data", || {
    api_client::get_user_data(user_id)
})?;
```

## Related Topics

- [Runtime Handler](runtime.md) - For runtime information and operations
- [Message Server Handler](message-server.md) - For actor-to-actor communication
- [State Management](../core-concepts/state-management.md) - For state chain integration

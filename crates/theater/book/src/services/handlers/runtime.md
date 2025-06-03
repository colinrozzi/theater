# Runtime Handler

The Runtime Handler provides actors with information about and control over their runtime environment in Theater. It enables actors to access runtime metadata, manage their lifecycle, and interact with the Theater runtime system.

## Overview

The Runtime Handler implements the `theater:simple/runtime` interface, providing actors with the ability to:

1. Access information about themselves and the runtime
2. Control their lifecycle
3. Get system and environment information
4. Record custom metrics and events
5. Manage runtime resources

## Configuration

To use the Runtime Handler, add it to your actor's manifest:

```toml
[[handlers]]
type = "runtime"
config = {}
```

The Runtime Handler doesn't currently require any specific configuration parameters.

## Interface

The Runtime Handler is defined using the following WIT interface:

```wit
interface runtime {
    // Get the actor's unique ID
    get-actor-id: func() -> string;
    
    // Get the actor's name
    get-actor-name: func() -> string;
    
    // Get current timestamp (milliseconds since epoch)
    get-current-time: func() -> u64;
    
    // Get environment variable value
    get-env: func(name: string) -> option<string>;
    
    // Log a message with specified level
    log: func(level: string, message: string) -> result<_, string>;
    
    // Record a custom metric
    record-metric: func(name: string, value: float64) -> result<_, string>;
    
    // Record a custom event
    record-event: func(event-type: string, data: list<u8>) -> result<_, string>;
    
    // Get theater version
    get-theater-version: func() -> string;
    
    // Get system information
    get-system-info: func() -> system-info;
    
    // Runtime statistics and information
    record system-info {
        hostname: string,
        os-type: string,
        os-release: string,
        cpu-count: u32,
        memory-total: u64,
        memory-available: u64,
        uptime: u64,
    }
}
```

## Runtime Information

### Getting Actor Information

To get the actor's ID and name:

```rust
let actor_id = runtime::get_actor_id();
let actor_name = runtime::get_actor_name();

println!("Actor ID: {}", actor_id);
println!("Actor name: {}", actor_name);
```

### Getting Current Time

To get the current time (milliseconds since epoch):

```rust
let now = runtime::get_current_time();
println!("Current time: {} ms", now);
```

### Getting Theater Version

To get the current Theater runtime version:

```rust
let version = runtime::get_theater_version();
println!("Theater version: {}", version);
```

### Getting System Information

To get information about the system:

```rust
let system_info = runtime::get_system_info();

println!("System Information:");
println!("Hostname: {}", system_info.hostname);
println!("OS Type: {}", system_info.os_type);
println!("OS Release: {}", system_info.os_release);
println!("CPU Count: {}", system_info.cpu_count);
println!("Total Memory: {} bytes", system_info.memory_total);
println!("Available Memory: {} bytes", system_info.memory_available);
println!("System Uptime: {} seconds", system_info.uptime);
```

### Getting Environment Variables

To access environment variables:

```rust
if let Some(log_level) = runtime::get_env("LOG_LEVEL") {
    println!("Log level from environment: {}", log_level);
} else {
    println!("LOG_LEVEL environment variable not set");
}
```

## Logging and Events

### Logging Messages

To log messages at different levels:

```rust
// Log with different levels
runtime::log("debug", "This is a debug message").unwrap();
runtime::log("info", "This is an info message").unwrap();
runtime::log("warn", "This is a warning message").unwrap();
runtime::log("error", "This is an error message").unwrap();
```

### Recording Custom Metrics

To record custom metrics:

```rust
// Record a performance metric
runtime::record_metric("request_duration_ms", 42.5).unwrap();

// Record a counter
runtime::record_metric("requests_processed", 1.0).unwrap();

// Record memory usage
runtime::record_metric("memory_usage_bytes", 1024.0 * 1024.0).unwrap();
```

### Recording Custom Events

To record custom events:

```rust
// Record a simple event
let event_data = b"User logged in".to_vec();
runtime::record_event("user_login", event_data).unwrap();

// Record a structured event
let complex_event = serde_json::json!({
    "action": "item_purchase",
    "user_id": "user123",
    "item_id": "item456",
    "amount": 29.99,
    "currency": "USD"
});
let event_bytes = serde_json::to_vec(&complex_event).unwrap();
runtime::record_event("purchase", event_bytes).unwrap();
```

## State Chain Integration

All runtime operations are recorded in the actor's state chain, creating a verifiable history. The chain events include:

1. **RuntimeOperation**: Records runtime operations like environment variable access or system info requests
2. **CustomEvent**: Records user-defined events with their data
3. **LogEvent**: Records log messages with their level
4. **MetricEvent**: Records custom metrics with their values

## Error Handling

The Runtime Handler provides error information for various failure scenarios:

1. **Log Errors**: When logging fails
2. **Metric Errors**: When metric recording fails
3. **Event Errors**: When custom event recording fails
4. **Environment Errors**: When environment variable access fails

## Security Considerations

When using the Runtime Handler, consider the following security aspects:

1. **Environment Variables**: Be careful with sensitive environment variables
2. **Logging**: Don't log sensitive data like passwords or tokens
3. **Metrics**: Avoid using personally identifiable information in metric names
4. **Custom Events**: Be mindful of the data included in custom events
5. **System Information**: Consider what system information is exposed to actors

## Implementation Details

Under the hood, the Runtime Handler:

1. Provides a bridge between WebAssembly actors and the host runtime
2. Translates WIT interface calls to host runtime operations
3. Records all operations in the state chain
4. Manages access to system resources and information
5. Interacts with the logging and metrics subsystems

## Use Cases

### Application Monitoring

```rust
// Record application health metrics periodically
fn record_health_metrics() -> Result<(), String> {
    // Get system information
    let system_info = runtime::get_system_info();
    
    // Record memory metrics
    let memory_used = system_info.memory_total - system_info.memory_available;
    runtime::record_metric("memory_used_bytes", memory_used as f64)?;
    
    // Record memory percentage
    let memory_percentage = (memory_used as f64 / system_info.memory_total as f64) * 100.0;
    runtime::record_metric("memory_usage_percent", memory_percentage)?;
    
    // Record CPU metrics (application-specific)
    let cpu_usage = calculate_cpu_usage();
    runtime::record_metric("cpu_usage_percent", cpu_usage)?;
    
    // Log status
    runtime::log("info", &format!("Health metrics recorded: Memory {}%, CPU {}%", 
                                memory_percentage, cpu_usage))?;
    
    Ok(())
}
```

### Structured Logging

```rust
// Structured logging helper
fn log_structured(level: &str, message: &str, context: &serde_json::Value) -> Result<(), String> {
    let log_entry = serde_json::json!({
        "message": message,
        "timestamp": runtime::get_current_time(),
        "actor": {
            "id": runtime::get_actor_id(),
            "name": runtime::get_actor_name(),
        },
        "context": context
    });
    
    let log_message = serde_json::to_string(&log_entry)
        .map_err(|e| format!("Failed to serialize log: {}", e))?;
    
    runtime::log(level, &log_message)
}

// Usage
fn process_request(request: &Request) -> Result<Response, String> {
    // Log request received
    log_structured("info", "Request received", &serde_json::json!({
        "request_id": request.id,
        "client_ip": request.client_ip,
        "method": request.method,
        "path": request.path
    }))?;
    
    // Process request
    let start_time = runtime::get_current_time();
    let result = handle_request(request);
    let duration = runtime::get_current_time() - start_time;
    
    // Record processing time
    runtime::record_metric("request_duration_ms", duration as f64)?;
    
    // Log result
    match &result {
        Ok(response) => {
            log_structured("info", "Request completed", &serde_json::json!({
                "request_id": request.id,
                "status": response.status,
                "duration_ms": duration
            }))?;
        },
        Err(error) => {
            log_structured("error", "Request failed", &serde_json::json!({
                "request_id": request.id,
                "error": error,
                "duration_ms": duration
            }))?;
        }
    }
    
    result
}
```

### Feature Flags

```rust
// Check if a feature is enabled via environment variables
fn is_feature_enabled(feature_name: &str) -> bool {
    let env_var_name = format!("FEATURE_{}", feature_name.to_uppercase());
    
    match runtime::get_env(&env_var_name) {
        Some(value) => {
            match value.to_lowercase().as_str() {
                "true" | "yes" | "1" => true,
                _ => false,
            }
        },
        None => false,
    }
}

// Usage
fn process_request(request: &Request) -> Response {
    if is_feature_enabled("new_ui") {
        // Use new UI processing
        process_with_new_ui(request)
    } else {
        // Use old UI processing
        process_with_old_ui(request)
    }
}
```

## Best Practices

1. **Consistent Logging**: Use consistent log levels and formats
2. **Meaningful Metrics**: Design metrics that provide actionable insights
3. **Error Handling**: Always handle errors from runtime functions
4. **Resource Usage**: Be mindful of resource usage in metrics collection
5. **Security**: Never log sensitive information

## Performance Considerations

1. **Logging Overhead**: Excessive logging can impact performance
2. **Metric Cardinality**: Too many unique metric names can cause issues
3. **Event Size**: Large event payloads may impact performance
4. **System Info Calls**: Frequent system info calls may have overhead

## Related Topics

- [Message Server Handler](message-server.md) - For actor-to-actor communication
- [Supervisor Handler](supervisor.md) - For parent-child actor relationships
- [Timing Handler](timing.md) - For timing and scheduling operations
- [State Management](../core-concepts/state-management.md) - For state chain integration

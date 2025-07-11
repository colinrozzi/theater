# Handler Setup Error Chain Propagation

**Date**: 2025-07-11  
**Status**: Implemented  
**Type**: Enhancement  
**Scope**: Host Handlers, Event Chain, Error Handling  

## Description

This proposal outlines the implementation of comprehensive error chain propagation for handler setup functions. Previously, handler setup errors were logged but not recorded in the actor's event chain, creating gaps in traceability and making debugging production issues significantly more difficult.

The implementation replaces all panic-prone `.expect()` calls in handler setup functions with proper error handling that records detailed events to the actor's event chain, ensuring complete auditability of the actor lifecycle from creation through runtime operations.

### Why This Change Was Necessary

- **Audit Trail Gaps**: Handler setup errors were logged but not recorded in the event chain, breaking traceability
- **Production Instability**: `.expect()` calls would panic on setup failures, crashing actors instead of graceful degradation  
- **Debugging Difficulty**: No way to trace which specific setup step failed when actors wouldn't start
- **Inconsistent Error Handling**: Runtime operations recorded events but setup operations did not
- **Missing Observability**: External monitoring systems couldn't observe setup failures through the event chain

### Expected Benefits

- Complete audit trail covering the entire actor lifecycle
- Graceful error handling instead of panics during setup failures
- Detailed error information including specific failure points and error messages
- Consistent event recording patterns across all actor operations
- Better debugging capabilities for production issues
- Foundation for replay and deterministic debugging systems
- Improved reliability and production readiness

### Potential Risks

- Slight performance overhead from additional event recording during setup
- Increased event chain size due to more detailed setup tracking
- Potential for event recording failures during error conditions (mitigated by using established patterns)

## Technical Approach

### 1. Event Type Extension

Added new event types to `MessageEventData` enum for tracking handler setup lifecycle:

```rust
// Handler setup events
HandlerSetupStart,
HandlerSetupSuccess, 
HandlerSetupError {
    error: String,
    step: String,
},
LinkerInstanceSuccess,
FunctionSetupStart {
    function_name: String,
},
FunctionSetupSuccess {
    function_name: String,
},
```

### 2. Error Handling Pattern

**Before** (Panic-prone):
```rust
let mut interface = actor_component
    .linker
    .instance("theater:simple/message-server-host")
    .expect("Could not instantiate theater:simple/message-server-host");
```

**After** (Event-recording):
```rust
let mut interface = match actor_component
    .linker
    .instance("theater:simple/message-server-host")
{
    Ok(interface) => {
        // Record successful linker instance creation
        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: EventData::Message(MessageEventData::LinkerInstanceSuccess),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Successfully created linker instance".to_string()),
        });
        interface
    }
    Err(e) => {
        // Record the specific error where it happens
        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: EventData::Message(MessageEventData::HandlerSetupError {
                error: e.to_string(),
                step: "linker_instance".to_string(),
            }),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some(format!("Failed to create linker instance: {}", e)),
        });
        return Err(anyhow::anyhow!("Could not instantiate theater:simple/message-server-host: {}", e));
    }
};
```

### 3. Comprehensive Function Coverage

Applied the pattern to all setup operations:

**Linker Operations**:
- Linker instance creation
- Function wrapper setup for all handler functions

**Function Registration**:
- Export function registration
- Interface binding

**Error Categorization**:
- Step-specific error identification (`linker_instance`, `send_function_wrap`, etc.)
- Detailed error messages with context
- Success confirmations for positive path tracking

## Implementation Pattern

This pattern should be applied to **ALL** handler setup functions. Here's the canonical template:

### Step 1: Record Setup Start
```rust
pub async fn setup_host_functions(
    &mut self,
    actor_component: &mut ActorComponent,
) -> Result<()> {
    // Record setup start
    actor_component.actor_store.record_event(ChainEventData {
        event_type: "{handler-name}-setup".to_string(),
        data: EventData::{HandlerType}(HandlerEventData::HandlerSetupStart),
        timestamp: chrono::Utc::now().timestamp_millis() as u64,
        description: Some("Starting {handler} host function setup".to_string()),
    });
```

### Step 2: Replace .expect() with Match Statements
```rust
    // BEFORE: .expect("Error message")
    // AFTER:
    let result = match potentially_failing_operation() {
        Ok(success_value) => {
            // Record success event
            actor_component.actor_store.record_event(ChainEventData {
                event_type: "{handler-name}-setup".to_string(),
                data: EventData::{HandlerType}(HandlerEventData::{OperationSuccess}),
                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                description: Some("Successfully completed {operation}".to_string()),
            });
            success_value
        }
        Err(e) => {
            // Record error event with specific step information
            actor_component.actor_store.record_event(ChainEventData {
                event_type: "{handler-name}-setup".to_string(),
                data: EventData::{HandlerType}(HandlerEventData::HandlerSetupError {
                    error: e.to_string(),
                    step: "{specific_operation_step}".to_string(),
                }),
                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                description: Some(format!("Failed to {operation}: {}", e)),
            });
            return Err(anyhow::anyhow!("Failed to {operation}: {}", e));
        }
    };
```

### Step 3: Record Overall Completion
```rust
    // Record overall setup completion
    actor_component.actor_store.record_event(ChainEventData {
        event_type: "{handler-name}-setup".to_string(),
        data: EventData::{HandlerType}(HandlerEventData::HandlerSetupSuccess),
        timestamp: chrono::Utc::now().timestamp_millis() as u64,
        description: Some("{Handler} host functions setup completed successfully".to_string()),
    });

    Ok(())
}
```

### Step 4: Add Required Event Types
For each handler, add to the corresponding event data enum:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum {HandlerName}EventData {
    // ... existing events ...
    
    // Handler setup events
    HandlerSetupStart,
    HandlerSetupSuccess,
    HandlerSetupError {
        error: String,
        step: String,
    },
    // Operation-specific success events
    {OperationName}Success,
    // Function-specific events
    FunctionSetupStart {
        function_name: String,
    },
    FunctionSetupSuccess {
        function_name: String,
    },
}
```

## Implementation Status

### ✅ Completed: MessageServerHost
- **File**: `src/host/message_server.rs`
- **Event Types**: Added to `src/events/message.rs`
- **Functions Modified**: 15 total
  - Linker instance creation
  - 8 function wrapper setups (`send`, `request`, `list-outstanding-requests`, etc.)
  - 5 export function registrations (`handle-send`, `handle-request`, etc.)
  - 1 async operation error handling
- **Testing**: All 85 tests passing
- **Status**: Production ready

### 🚧 Remaining Handlers to Implement

Apply the same pattern to these handlers:

1. **FileSystemHost** (`src/host/filesystem.rs`)
   - Event types: `src/events/filesystem.rs`
   - High priority due to security implications

2. **HttpClientHost** (`src/host/http_client.rs`)
   - Event types: `src/events/http.rs`
   - Medium priority

3. **ProcessHost** (`src/host/process.rs`)
   - Event types: `src/events/process.rs`
   - High priority due to security implications

4. **SupervisorHost** (`src/host/supervisor.rs`)
   - Event types: `src/events/supervisor.rs`
   - High priority for actor orchestration

5. **StoreHost** (`src/host/store.rs`)
   - Event types: `src/events/store.rs`
   - Medium priority

6. **TimingHost** (`src/host/timing.rs`)
   - Event types: `src/events/timing/mod.rs`
   - Low priority

7. **RandomHost** (`src/host/random.rs`)
   - Event types: `src/events/random.rs`
   - Low priority

8. **EnvironmentHost** (`src/host/environment.rs`)
   - Event types: `src/events/environment.rs`
   - Low priority

9. **RuntimeHost** (`src/host/runtime.rs`)
   - Event types: `src/events/runtime.rs`
   - Medium priority

10. **HttpFramework** (`src/host/framework/`)
    - Event types: `src/events/http.rs`
    - Medium priority

## Event Chain Coverage After Full Implementation

```
Actor Lifecycle Event Coverage:
├── Actor Creation ✅
├── Permission Validation ✅  
├── Handler Creation ✅
├── Component Creation ✅
├── Host Function Setup ✅ (NEW!)
│   ├── Linker Instance ✅
│   ├── Function Wrappers ✅
│   └── Export Functions ✅
├── Component Instantiation ✅
├── Export Function Setup ✅
├── State Initialization ✅
└── Runtime Operations ✅
```

## Performance Considerations

- **Setup Events**: ~10-20 additional events per actor during setup
- **Memory Impact**: ~1-2KB additional chain data per actor
- **CPU Impact**: Negligible (<1ms additional setup time)
- **Network Impact**: None (events are local until chain sync)

## Testing Strategy

For each handler implementation:

1. **Unit Tests**: Verify error conditions trigger proper events
2. **Integration Tests**: Confirm setup failures are recorded in chain
3. **Error Injection Tests**: Deliberately cause setup failures and verify chain recording
4. **Performance Tests**: Ensure setup time remains acceptable

## Debugging Improvements

With this implementation, debugging actor setup failures becomes dramatically easier:

**Before**:
```
ERROR: Failed to set up host functions for handler MessageServer: ...
```

**After**:
```
Chain Event: message-server-setup/HandlerSetupStart
Chain Event: message-server-setup/LinkerInstanceSuccess  
Chain Event: message-server-setup/FunctionSetupStart { function_name: "send" }
Chain Event: message-server-setup/HandlerSetupError { 
    error: "invalid function signature", 
    step: "send_function_wrap" 
}
```

## Migration Guide

To implement this pattern for a new handler:

1. **Identify all `.expect()` calls** in the handler's `setup_host_functions` method
2. **Add event types** to the handler's event data enum  
3. **Replace `.expect()` calls** with the match pattern shown above
4. **Add setup start/success events** at function boundaries
5. **Test error conditions** to ensure events are recorded
6. **Update handler documentation** to mention event recording

## Conclusion

This implementation provides the foundation for complete actor lifecycle traceability. The Message Server handler serves as the canonical example, and the same pattern should be applied consistently across all handlers to ensure comprehensive error tracking and debugging capabilities.

The pattern is production-ready, tested, and provides significant improvements to system observability while maintaining backward compatibility and performance characteristics.

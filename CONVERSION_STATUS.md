# Handler Event Recording Pattern Conversion Status

## Summary

The task is to update 7 handler crates from the old `ChainEventData` event recording pattern to the new `record_handler_event()` pattern.

## Handlers That Need Conversion

All 7 handlers have their event modules properly set up with handler-specific event types:

1. **theater-handler-http-framework** - Uses `HttpFrameworkEventData`
2. **theater-handler-http-client** - Uses `HttpEventData`
3. **theater-handler-message-server** - Uses `MessageEventData`
4. **theater-handler-process** - Uses `ProcessEventData`
5. **theater-handler-random** - Uses `RandomEventData`
6. **theater-handler-store** - Uses `StoreEventData`
7. **theater-handler-supervisor** - Uses `SupervisorEventData`

## Required Changes

### 1. Update Handler Trait Method Signatures

Change:
```rust
fn setup_host_functions(&mut self, actor_component: &mut ActorComponent) -> Result<()>
fn add_export_functions(&self, actor_instance: &mut ActorInstance) -> Result<()>
```

To:
```rust
fn setup_host_functions(&mut self, actor_component: &mut ActorComponent<E>) -> Result<()>
fn add_export_functions(&self, actor_instance: &mut ActorInstance<E>) -> Result<()>
```

### 2. Convert Event Recording Calls

**OLD Pattern:**
```rust
ctx.data_mut().record_event(ChainEventData {
    event_type: "handler/operation".to_string(),
    data: EventData::HandlerType(HandlerEventData::Event {
        field: value
    }),
    timestamp: chrono::Utc::now().timestamp_millis() as u64,
    description: Some("description".to_string()),
});
```

**NEW Pattern:**
```rust
ctx.data_mut().record_handler_event(
    "handler/operation".to_string(),
    HandlerEventData::Event {
        field: value
    },
    Some("description".to_string()),
);
```

## Event Counts Per Handler

- **http-framework**: 15 events to convert
- **http-client**: 10 events to convert
- **message-server**: 40 events to convert
- **process**: 10 events to convert
- **random**: 13 events to convert
- **store**: 32 events to convert
- **supervisor**: 37 events to convert

**Total**: 157 event recording calls to convert

## Challenges Encountered

1. **Multi-line structures**: Event data often spans multiple lines with nested braces
2. **Variable event data**: Some handlers build event data in variables before recording
3. **Regex limitations**: Simple regex patterns fail with nested structures
4. **Syntax preservation**: Need to maintain exact formatting and avoid breaking delimiters

## Recommended Approach

Given the complexity and number of conversions, manual conversion using targeted Edit calls for each handler is most reliable. The pattern is consistent across all handlers - it's a mechanical transformation that needs to preserve structure.

## Next Steps

1. Start with the simplest handlers (http-client, process) that have fewer events
2. Use precise Edit calls to replace each event recording block
3. Test compilation after each handler
4. Move to more complex handlers (store, message-server, supervisor)
5. Verify all handlers compile successfully
6. Run integration tests to ensure functionality is preserved

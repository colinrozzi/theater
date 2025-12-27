# Type-Safe Events Example

This example demonstrates Theater's type-safe event system where handler events are decoupled from the core runtime and composed at the application level.

## Key Concepts

### 1. **Handler Events Live in Handler Crates**

Each handler defines its own event types:
```rust
// In theater-handler-environment/src/events.rs
pub enum EnvironmentEventData {
    GetVar { ... },
    PermissionDenied { ... },
    // ...
}
```

### 2. **Applications Compose Events**

Applications define which handlers they use with a newtype wrapper:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MyHandlerEvents {
    Environment(EnvironmentEventData),
    // Only include handlers you use!
}

// Use a newtype wrapper to enable From implementations (Rust orphan rules)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MyAppEvents(TheaterEvents<MyHandlerEvents>);
```

### 3. **Compiler Enforces Type Safety**

Applications must implement `From` for each handler and core event type:
```rust
// Core event types
impl From<RuntimeEventData> for MyAppEvents {
    fn from(event: RuntimeEventData) -> Self {
        MyAppEvents(TheaterEvents::Runtime(event))
    }
}

// Handler event types
impl From<EnvironmentEventData> for MyAppEvents {
    fn from(event: EnvironmentEventData) -> Self {
        MyAppEvents(TheaterEvents::Handler(MyHandlerEvents::Environment(event)))
    }
}
```

**If you use a handler without implementing `From`, the code won't compile!**

**Note:** The newtype wrapper `MyAppEvents(...)` is required to satisfy Rust's orphan rules, which prevent implementing external traits (`From`) for external types (`TheaterEvents`).

### 4. **Runtime is Generic**

The runtime is instantiated with your concrete event type:
```rust
let runtime = TheaterRuntime::<MyAppEvents>::new(...).await?;
```

## Benefits

✅ **Modular** - Handlers are truly independent, no circular dependencies
✅ **Type-Safe** - Compile-time enforcement of event conversion
✅ **Minimal** - Only include events for handlers you use
✅ **Zero-Cost** - All type checking happens at compile time
✅ **Decoupled** - Handler crates don't depend on runtime's event enum

## Running the Example

```bash
cd examples/type-safe-events
cargo run
```

## Architecture Diagram

```
┌─────────────────────────────────────────┐
│         Handler Crates                   │
│  (Define their own event types)         │
├─────────────────────────────────────────┤
│  theater-handler-environment             │
│    └─ EnvironmentEventData              │
│  theater-handler-timing                  │
│    └─ TimingEventData                    │
│  theater-handler-http-framework          │
│    └─ HttpEventData                      │
└─────────────────────────────────────────┘
                  ↓
┌─────────────────────────────────────────┐
│      Application Layer                   │
│  (Composes events from handlers used)   │
├─────────────────────────────────────────┤
│  enum MyHandlerEvents {                  │
│      Environment(EnvironmentEventData),  │
│      Timing(TimingEventData),            │
│  }                                       │
│                                          │
│  type MyAppEvents =                      │
│      TheaterEvents<MyHandlerEvents>      │
│                                          │
│  impl From<EnvironmentEventData> ...     │
│  impl From<TimingEventData> ...          │
└─────────────────────────────────────────┘
                  ↓
┌─────────────────────────────────────────┐
│     Theater Runtime<MyAppEvents>         │
│  (Generic over application event type)  │
├─────────────────────────────────────────┤
│  • ActorStore<MyAppEvents>               │
│  • ActorComponent<MyAppEvents>           │
│  • ActorInstance<MyAppEvents>            │
│  • record_handler_event<H>()             │
│    where MyAppEvents: From<H>            │
└─────────────────────────────────────────┘
```

## Compile-Time Safety Example

```rust
// ✓ This compiles - From trait implemented
handler_registry.register(EnvironmentHandler::new(...));

// ✗ This won't compile - missing From<TimingEventData>
handler_registry.register(TimingHandler::new());
//                        ^^^^^^^^^^^^^^^^^^^^
// Error: the trait `From<TimingEventData>` is not implemented for `MyAppEvents`
```

The compiler catches missing event conversions at build time!

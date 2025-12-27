//! # Type-Safe Events Example
//!
//! This example demonstrates Theater's type-safe event system where:
//! 1. Applications define only the handler events they need
//! 2. The runtime is generic over event types
//! 3. Compile-time safety ensures proper event conversion
//!
//! ## Architecture
//!
//! ```
//! Application defines:
//!   MyHandlerEvents enum (only handlers used)
//!     ↓
//!   Composed with TheaterEvents<MyHandlerEvents>
//!     ↓
//!   Implement From traits for each handler
//!     ↓
//!   Runtime instantiated with concrete type
//! ```

use serde::{Deserialize, Serialize};
use theater::events::TheaterEvents;
use theater::handler::HandlerRegistry;
use theater_handler_environment::EnvironmentEventData;

// Step 1: Define your application's handler events
// Only include the handlers you're actually using!
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MyHandlerEvents {
    /// Environment variable access events
    Environment(EnvironmentEventData),

    // Could add more handlers here as needed:
    // Timing(TimingEventData),
    // Http(HttpEventData),
    // etc.
}

// Step 2: Create your application's complete event type using a newtype wrapper
// We use a newtype (struct wrapper) to make this type local, which allows us to
// implement From traits (required by Rust's orphan rules)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MyAppEvents(TheaterEvents<MyHandlerEvents>);

// Step 3: Implement From traits for core event types
// These are needed because we're using a newtype wrapper
impl From<theater::events::runtime::RuntimeEventData> for MyAppEvents {
    fn from(event: theater::events::runtime::RuntimeEventData) -> Self {
        MyAppEvents(TheaterEvents::Runtime(event))
    }
}

impl From<theater::events::wasm::WasmEventData> for MyAppEvents {
    fn from(event: theater::events::wasm::WasmEventData) -> Self {
        MyAppEvents(TheaterEvents::Wasm(event))
    }
}

impl From<theater::events::theater_runtime::TheaterRuntimeEventData> for MyAppEvents {
    fn from(event: theater::events::theater_runtime::TheaterRuntimeEventData) -> Self {
        MyAppEvents(TheaterEvents::TheaterRuntime(event))
    }
}

// Step 4: Implement From traits for each handler you use
// The compiler will error if you use a handler but forget this!
// The newtype wrapper makes this implementation valid under Rust's orphan rules
impl From<EnvironmentEventData> for MyAppEvents {
    fn from(event: EnvironmentEventData) -> Self {
        // Wrap environment events in our handler enum, then in TheaterEvents
        MyAppEvents(TheaterEvents::Handler(MyHandlerEvents::Environment(event)))
    }
}

// Note: If you added TimingEventData to MyHandlerEvents, you'd also need:
// impl From<TimingEventData> for MyAppEvents { ... }
// The compiler enforces this!

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    println!("🎉 Type-Safe Events Example");
    println!("============================\n");

    // Step 4: Create handler registry with your application's event type
    let mut handler_registry = HandlerRegistry::<MyAppEvents>::new();

    // Register environment handler
    use theater_handler_environment::EnvironmentHandler;
    use theater::config::actor_manifest::EnvironmentHandlerConfig;

    let env_config = EnvironmentHandlerConfig {
        allowed_vars: Some(vec!["PATH".to_string(), "HOME".to_string()]),
        denied_vars: None,
        allow_list_all: false,
        allowed_prefixes: None,
    };

    handler_registry.register(EnvironmentHandler::new(env_config, None));

    println!("✓ Registered EnvironmentHandler");

    // If you tried to use a handler without implementing From, you'd get a compile error!
    // For example, uncommenting this without adding From<TimingEventData> would fail:
    // handler_registry.register(TimingHandler::new());

    println!("✓ Handler registry created with type-safe event composition");

    // Step 5: Runtime would be instantiated with concrete event type
    // let runtime = TheaterRuntime::<MyAppEvents>::new(...).await?;

    println!("\n🎯 Key Benefits:");
    println!("  • Only includes events for handlers you use (smaller event enum)");
    println!("  • Compile-time safety - can't use handler without From impl");
    println!("  • Type-safe event recording in handlers");
    println!("  • Zero runtime overhead - all checked at compile time");
    println!("  • Handlers are truly decoupled from core runtime");

    println!("\n✨ Successfully demonstrated type-safe event composition!");

    Ok(())
}

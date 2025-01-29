use anyhow::Result;

pub mod actor_handle;
pub mod actor_runtime;
pub mod config;
pub mod host;
pub mod id; // Add the new id module
pub mod messages;
pub mod router;
mod store;
pub mod theater_runtime;
mod wasm;

pub use config::{HandlerConfig, HttpServerHandlerConfig, ManifestConfig, MessageServerConfig};
pub use id::TheaterId; // Expose TheaterId type
pub use store::ActorStore;
pub use wasm::{WasmActor, WasmError};


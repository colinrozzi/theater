//! # Theater Actor System
//!
//! Theater is a runtime for managing and executing WebAssembly actors in a distributed system.
//! It provides a framework for creating, managing, and communicating with actors that are
//! implemented as WebAssembly components.
//!
//! ## Core Features
//!
//! * **Actor Management**: Create, start, stop, and monitor actors
//! * **State Management**: Persistent actor state with event chain tracking
//! * **Message Passing**: Communication between actors and external systems
//! * **WebAssembly Integration**: Run actors as sandboxed WebAssembly components
//! * **Extensible Handlers**: Support for HTTP, WebSockets, and custom protocols
//!
//! ## Architecture
//!
//! Theater is built around these key components:
//!
//! * `TheaterRuntime`: The central runtime that manages the lifecycle of actors
//! * `ActorRuntime`: Manages the execution environment for a single actor
//! * `ActorExecutor`: Handles the actual execution of WebAssembly code
//! * `ActorHandle`: Provides an interface for interacting with actors
//! * `StateChain`: Tracks the history of state changes for an actor
//!
//! ## Example Usage
//!
//!
//! ## Security and Safety
//!
//! Theater runs actors in isolated WebAssembly environments with configurable resource limits
//! and capabilities. This provides strong security boundaries between actors and between
//! actors and the host system.

use anyhow::Result;

pub mod actor_executor;
pub mod actor_handle;
pub mod actor_runtime;
pub mod actor_store;
pub mod chain;
pub mod config;
pub mod events;
pub mod host;
pub mod id;
pub mod logging;
pub mod messages;
pub mod metrics;
pub mod shutdown;
pub mod store;
pub mod theater_runtime;
pub mod theater_server;
pub mod utils;
mod wasm;

pub use actor_store::ActorStore;
pub use chain::{ChainEvent, StateChain};
pub use config::{HandlerConfig, HttpServerHandlerConfig, ManifestConfig, MessageServerConfig};
pub use id::TheaterId;
pub use metrics::{ActorMetrics, MetricsCollector, OperationStats};
pub use wasm::{MemoryStats, WasmError};

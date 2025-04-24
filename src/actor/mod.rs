//! # Actor System
//!
//! The actor system is the core of the Theater runtime, providing the foundation
//! for executing WebAssembly components in an isolated, managed environment.
//!
//! This module contains all the components necessary for actor lifecycle management,
//! state handling, and operation execution. Together, these components form a robust
//! actor system with isolation, supervision, and fault tolerance capabilities.

pub mod handle;
pub mod runtime;
pub mod store;
pub mod types;

// Public re-exports
pub use handle::ActorHandle;
pub use runtime::ActorRuntime;
pub use runtime::StartActorResult;
pub use store::ActorStore;
pub use types::ActorError;
pub use types::ActorOperation;

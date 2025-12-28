//! Generated bindings from WASI Random WIT interfaces
//!
//! This module uses wasmtime::component::bindgen! to generate type-safe Host traits
//! from the WASI Random 0.2.0 WIT definitions.

use wasmtime::component::bindgen;

bindgen!({
    world: "random-handler-host",
    path: "wit",
    async: true,
    trappable_imports: true,
});

// Re-export the generated Host traits for easier access
pub use wasi::random::random::Host as RandomHost;
pub use wasi::random::insecure::Host as InsecureHost;
pub use wasi::random::insecure_seed::Host as InsecureSeedHost;

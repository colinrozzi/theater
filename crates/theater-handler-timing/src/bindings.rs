//! Generated bindings from WASI Clocks WIT interfaces
//!
//! This module uses wasmtime::component::bindgen! to generate type-safe Host traits
//! from the WASI Clocks 0.2.0 WIT definitions.

use wasmtime::component::bindgen;

bindgen!({
    world: "timing-handler-host",
    path: "wit",
    with: {
        // Map the pollable resource to our backing type
        "wasi:io/poll/pollable": crate::Pollable,
    },
    async: true,
    trappable_imports: true,
});

// Re-export the generated Host traits for easier access
pub use wasi::clocks::wall_clock::Host as WallClockHost;
pub use wasi::clocks::monotonic_clock::Host as MonotonicClockHost;
pub use wasi::io::poll::Host as PollHost;
pub use wasi::io::poll::HostPollable;

// Re-export types
pub use wasi::clocks::wall_clock::Datetime;

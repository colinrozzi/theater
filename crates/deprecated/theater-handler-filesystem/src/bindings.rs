//! Bindgen-generated bindings for WASI filesystem interfaces
//!
//! This module uses wasmtime's bindgen! macro to generate type-safe Host traits
//! from the WASI filesystem WIT definitions.

use wasmtime::component::bindgen;

bindgen!({
    world: "filesystem-handler-host",
    path: "wit",
    with: {
        // Map WASI filesystem resources to our backing types
        "wasi:filesystem/types/descriptor": crate::types::Descriptor,
        "wasi:filesystem/types/directory-entry-stream": crate::types::DirectoryEntryStream,

        // Map WASI IO resources to types from theater_handler_io
        "wasi:io/streams/input-stream": theater_handler_io::InputStream,
        "wasi:io/streams/output-stream": theater_handler_io::OutputStream,
        "wasi:io/error/error": theater_handler_io::IoError,
    },
    async: true,
    trappable_imports: true,
});

// Re-export the generated Host traits for convenience
pub use wasi::filesystem::types::Host as FilesystemTypesHost;
pub use wasi::filesystem::types::HostDescriptor;
pub use wasi::filesystem::types::HostDirectoryEntryStream;
pub use wasi::filesystem::preopens::Host as PreopensHost;

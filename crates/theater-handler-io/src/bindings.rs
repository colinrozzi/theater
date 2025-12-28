//! Bindgen-generated bindings for WASI I/O and CLI interfaces
//!
//! This module uses wasmtime's bindgen! macro to generate type-safe Host traits
//! from the WASI I/O and CLI WIT definitions.

use wasmtime::component::bindgen;

bindgen!({
    world: "io-handler-host",
    path: "wit",
    with: {
        // Map WASI I/O resources to our backing types
        "wasi:io/error/error": crate::IoError,
        "wasi:io/streams/input-stream": crate::InputStream,
        "wasi:io/streams/output-stream": crate::OutputStream,
        "wasi:io/poll/pollable": crate::poll::IoHandlerPollable,
    },
    async: true,
    trappable_imports: true,
});

// Re-export the generated Host traits for convenience
pub use wasi::io::error::Host as ErrorHost;
pub use wasi::io::error::HostError;
pub use wasi::io::poll::Host as PollHost;
pub use wasi::io::poll::HostPollable;
pub use wasi::io::streams::Host as StreamsHost;
pub use wasi::io::streams::HostInputStream;
pub use wasi::io::streams::HostOutputStream;

// Re-export CLI interface Host traits
pub use wasi::cli::stdin::Host as StdinHost;
pub use wasi::cli::stdout::Host as StdoutHost;
pub use wasi::cli::stderr::Host as StderrHost;
pub use wasi::cli::environment::Host as EnvironmentHost;
pub use wasi::cli::exit::Host as ExitHost;
pub use wasi::cli::terminal_input::Host as TerminalInputHost;
pub use wasi::cli::terminal_input::HostTerminalInput;
pub use wasi::cli::terminal_output::Host as TerminalOutputHost;
pub use wasi::cli::terminal_output::HostTerminalOutput;
pub use wasi::cli::terminal_stdin::Host as TerminalStdinHost;
pub use wasi::cli::terminal_stdout::Host as TerminalStdoutHost;
pub use wasi::cli::terminal_stderr::Host as TerminalStderrHost;

// Re-export types
pub use wasi::io::streams::StreamError;

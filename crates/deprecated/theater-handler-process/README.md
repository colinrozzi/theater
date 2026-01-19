# Theater Process Handler

OS process spawning and management handler for the Theater WebAssembly runtime.

## Overview

The Process Handler provides comprehensive OS process spawning and management capabilities to WebAssembly actors in the Theater system. It enables actors to spawn external processes, manage their I/O streams, monitor their lifecycle, and control their execution with permission-based access control.

## Features

- **Process Spawning**: Spawn OS processes with full configuration control
- **I/O Management**: Async stdin/stdout/stderr handling with multiple processing modes
- **Multiple Output Modes**:
  - Raw: Direct byte stream
  - Line-by-Line: Buffered line reading
  - JSON: Parse and validate JSON objects
  - Chunked: Fixed-size chunks
- **Process Lifecycle**: Monitor process state, exit codes, and execution time
- **Execution Timeouts**: Automatic process termination after specified duration
- **Permission Control**: Configurable process spawn permissions and limits
- **Complete Auditability**: All operations recorded in event chains

## Operations

### Process Management

- `os-spawn` - Spawn a new OS process with full configuration
- `os-status` - Get current process status (running, exit code, start time)
- `os-kill` - Terminate a running process
- `os-signal` - Send signals to processes (platform-specific)

### I/O Operations

- `os-write-stdin` - Write data to process stdin

### Export Functions (Callbacks)

Actors must implement these functions to receive process events:

- `handle-stdout` - Receive stdout data from processes
- `handle-stderr` - Receive stderr data from processes
- `handle-exit` - Receive process exit notifications

## Configuration

```rust
use theater_handler_process::ProcessHandler;
use theater::config::actor_manifest::ProcessHostConfig;
use theater::actor::handle::ActorHandle;

let config = ProcessHostConfig {
    max_processes: 10,           // Maximum concurrent processes
    max_output_buffer: 8192,     // Maximum output buffer size
    allowed_programs: None,      // Whitelist of allowed programs (None = all)
    allowed_paths: None,         // Whitelist of allowed working directories
};

let (operation_tx, _) = tokio::sync::mpsc::channel(100);
let (info_tx, _) = tokio::sync::mpsc::channel(100);
let (control_tx, _) = tokio::sync::mpsc::channel(100);
let actor_handle = ActorHandle::new(operation_tx, info_tx, control_tx);

let handler = ProcessHandler::new(config, actor_handle, None);
```

## Process Configuration

When spawning a process, you can configure:

```rust
ProcessConfig {
    program: String,              // Executable path
    args: Vec<String>,            // Command line arguments
    cwd: Option<String>,          // Working directory
    env: Vec<(String, String)>,   // Environment variables
    buffer_size: u32,             // I/O buffer size
    stdout_mode: OutputMode,      // How to process stdout
    stderr_mode: OutputMode,      // How to process stderr
    chunk_size: Option<u32>,      // Chunk size for chunked mode
    execution_timeout: Option<u64>, // Timeout in seconds
}
```

## Output Modes

### Raw Mode
Direct byte stream - data is sent as soon as it's read from the process.

### Line-by-Line Mode
Data is buffered and sent line by line (newline-delimited). Useful for processing line-oriented output.

### JSON Mode
Expects newline-delimited JSON objects. Each line is validated as JSON before being sent to the actor.

### Chunked Mode
Fixed-size chunks. Useful for binary data or when you need predictable chunk sizes.

## Security & Permissions

The Process Handler integrates with Theater's permission system:

- **Process Limits**: Limit the number of concurrent processes
- **Program Whitelist**: Restrict which programs can be executed
- **Path Whitelist**: Restrict working directories
- **Complete Audit Trail**: All spawn attempts, I/O operations, and terminations are recorded

## Event Recording

Every operation records detailed events:

- **Setup Events**: Handler initialization
- **Spawn Events**: Process spawn attempts and results
- **I/O Events**: stdin writes, stdout/stderr reads
- **Lifecycle Events**: Process state changes and exits
- **Error Events**: Detailed error information with operation context
- **Permission Events**: Permission checks and denials

## Architecture

### Process Lifecycle

1. **Spawn**: Process is started with tokio::process::Command
2. **I/O Setup**: Stdin/stdout/stderr pipes are created and monitored
3. **Async I/O Handling**: Separate tasks read stdout/stderr and send to actor
4. **Timeout Monitoring**: Optional timeout task kills process if it exceeds duration
5. **Exit Monitoring**: Task waits for process exit and notifies actor
6. **Cleanup**: Resources are released when process terminates

### Thread Safety

- Uses `Arc<Mutex<HashMap>>` for process management
- Careful lock management to avoid holding locks across await points
- All async operations are Send + 'static safe

## Example Usage in Actor Manifests

```toml
[[handlers]]
type = "process"
max_processes = 5
max_output_buffer = 4096
```

## Development

Run tests:
```bash
cargo test -p theater-handler-process
```

Build:
```bash
cargo build -p theater-handler-process
```

## Migration Status

This handler was migrated from the core `theater` crate (`src/host/process.rs`) to provide:

- ✅ Better modularity and separation of concerns
- ✅ Independent testing and development
- ✅ Clearer architecture and boundaries
- ✅ Simplified dependencies

**Original**: 1408 lines in `theater/src/host/process.rs`
**Migrated**: ~990 lines in standalone crate

## License

See the LICENSE file in the repository root.

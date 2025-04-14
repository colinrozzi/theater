# OS Process Handler

**Date:** 2025-04-14  
**Author:** Claude  
**Status:** Proposal  

## Summary

This change introduces a new handler type for spawning and communicating with operating system processes. This handler enables Theater actors to start external programs, interact with them via stdin/stdout/stderr, and monitor their lifecycle.

## Motivation

Currently, Theater actors cannot interact with external programs or system utilities. Adding this capability would open up a wide range of use cases:

1. Running specialized command-line tools and utilities 
2. Integrating with existing software that lacks WebAssembly support
3. Building processing pipelines that combine WebAssembly with native executables
4. Enabling actors to leverage platform-specific capabilities via external programs

## Design

### Interface

The OS Process Handler will provide a WIT interface for process management:

```wit
/// OS Process handler interface for managing operating system processes
interface process {
    use types.{state};

    /// Output processing mode
    enum output-mode {
        /// Process raw bytes without any special handling
        raw,
        /// Process output line by line
        line-by-line,
        /// Process output as JSON objects (newline-delimited)
        json,
        /// Process output in chunks of specified size
        chunked,
    }

    /// Configuration for a process
    record process-config {
        /// Executable path
        program: string,
        /// Command line arguments
        args: list<string>,
        /// Working directory (optional)
        cwd: option<string>,
        /// Environment variables
        env: list<tuple<string, string>>,
        /// Buffer size for stdout/stderr (in bytes)
        buffer-size: u32,
        /// How to process stdout
        stdout-mode: output-mode,
        /// How to process stderr
        stderr-mode: output-mode,
        /// Chunk size for chunked mode (in bytes)
        chunk-size: option<u32>,
    }

    /// Status of a running process
    record process-status {
        /// Process ID
        pid: u64,
        /// Whether the process is running
        running: bool,
        /// Exit code if not running (optional)
        exit-code: option<s32>,
        /// Start time in milliseconds since epoch
        start-time: u64,
        /// CPU usage percentage
        cpu-usage: float32,
        /// Memory usage in bytes
        memory-usage: u64,
    }

    /// Start a new OS process
    os-spawn: func(config: process-config) -> result<u64, string>;

    /// Write to the standard input of a process
    os-write-stdin: func(pid: u64, data: list<u8>) -> result<u32, string>;

    /// Get the status of a process
    os-status: func(pid: u64) -> result<process-status, string>;

    /// Send a signal to a process
    os-signal: func(pid: u64, signal: u32) -> result<unit, string>;

    /// Terminate a process
    os-kill: func(pid: u64) -> result<unit, string>;
}

/// Process handler export interface
interface process-handlers {
    /// Process output event
    handle-stdout: func(state: state, params: (u64, list<u8>)) -> result<(state,), string>;
    
    /// Process error output event
    handle-stderr: func(state: state, params: (u64, list<u8>)) -> result<(state,), string>;
    
    /// Process exit event
    handle-exit: func(state: state, params: (u64, s32)) -> result<(state,), string>;
}
```

### Implementation Details

The handler will be event-driven, notifying the actor when:
1. The process produces output on stdout or stderr
2. The process exits

Process output handling modes will enable different parsing strategies:
- Raw: Unprocessed byte streams
- Line-by-Line: Split output into complete lines
- JSON: Detect and parse complete JSON objects
- Chunked: Fixed-size chunks of data

### Configuration

In the actor manifest:

```toml
[[handlers]]
type = \"process\"
config = {
  max_processes = 10,
  max_output_buffer = 1048576,  # 1MB max output buffer per process
  allowed_programs = [
    \"/usr/bin/ls\",
    \"/usr/bin/grep\",
    \"/usr/bin/find\"
  ],
  allowed_paths = [
    \"/tmp\",
    \"/var/data\"
  ]
}
```

## Security Considerations

The OS Process Handler introduces significant security implications:

1. **Execution Control**: Limit which programs can be executed via allowlisting
2. **Path Restrictions**: Restrict working directories to safe locations
3. **Resource Limits**: Implement CPU/memory limits for spawned processes
4. **Buffer Management**: Prevent memory exhaustion from high-volume output
5. **Command Injection**: Validate all process parameters to prevent injection attacks
6. **Audit Logging**: Log all process operations for security auditing

## Files to Change

New files:
- `src/host/process.rs`: Main implementation of the ProcessHost
- `src/events/process.rs`: Process event definitions
- `wit/process.wit`: WIT interface definition for process management

Modified files:
- `src/host/handler.rs`: Add ProcessHost to Handler enum
- `src/host/mod.rs`: Export ProcessHost
- `src/events/mod.rs`: Include ProcessEventData
- `src/config.rs`: Add ProcessHostConfig

## Implementation Plan

1. Define the WIT interface for process management
2. Implement the ProcessHost handler with event-driven architecture
3. Add process management to the Handler enum
4. Implement ProcessHostConfig and security controls
5. Create tests for process lifecycle management
6. Update documentation with examples and security practices

## Testing Plan

1. Unit tests for process spawning, I/O, and termination
2. Integration tests with actors managing processes
3. Security tests verifying access controls
4. Performance tests under high output load
5. Error handling tests for invalid processes and commands

## Alternatives Considered

1. **Synchronous API**: A simpler API with synchronous calls was considered but rejected as it would block the actor
2. **Built-in Shell**: Implementing a shell parser was considered but rejected as too complex and insecure
3. **Process Pools**: A pool of reusable processes was considered but deemed unnecessary for the initial implementation

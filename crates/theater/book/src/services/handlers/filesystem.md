# Filesystem Handler

The Filesystem Handler provides actors with controlled access to the local filesystem. It enables reading and writing files, directory operations, and file metadata access, all while maintaining the Theater security model and state verification.

## Overview

The Filesystem Handler implements the `theater:simple/filesystem` interface, providing actors with the ability to:

1. Read and write files securely
2. Create and manage directories
3. Get file metadata
4. List directory contents
5. Safely access files within a specified path boundary

## Configuration

To use the Filesystem Handler, add it to your actor's manifest:

```toml
[[handlers]]
type = "filesystem"
config = { 
    path = "data/my-actor",
    allowed_commands = ["read", "write"]
}
```

Configuration options:

* `path`: (Optional) The base directory for all file operations, restricting access to this directory and its subdirectories
* `new_dir`: (Optional) If `true`, creates a new directory in /tmp/theater for the actor; if `false`, uses the specified path directly
* `allowed_commands`: (Optional) List of allowed filesystem operations; if not specified, all operations are allowed

## Interface

The Filesystem Handler is defined using the following WIT interface:

```wit
interface filesystem {
    read-file: func(path: string) -> result<list<u8>, string>;
    write-file: func(path: string, content: list<u8>) -> result<_, string>;
    append-file: func(path: string, content: list<u8>) -> result<_, string>;
    exists: func(path: string) -> result<bool, string>;
    is-file: func(path: string) -> result<bool, string>;
    is-dir: func(path: string) -> result<bool, string>;
    create-dir: func(path: string) -> result<_, string>;
    remove-file: func(path: string) -> result<_, string>;
    remove-dir: func(path: string) -> result<_, string>;
    list-dir: func(path: string) -> result<list<file-entry>, string>;
    metadata: func(path: string) -> result<file-metadata, string>;

    record file-entry {
        name: string,
        is-file: bool,
        is-dir: bool,
    }

    record file-metadata {
        name: string,
        path: string,
        size: u64,
        is-file: bool,
        is-dir: bool,
        created: option<u64>,
        modified: option<u64>,
        accessed: option<u64>,
    }
}
```

## File Operations

### Reading Files

To read a file:

```rust
match filesystem::read_file("config.json") {
    Ok(content) => {
        // Process file content
        let config: Config = serde_json::from_slice(&content).unwrap();
        // ...
    },
    Err(error) => {
        // Handle error
        println!("Failed to read file: {}", error);
    }
}
```

### Writing Files

To write a file:

```rust
let data = serde_json::to_vec(&config).unwrap();
match filesystem::write_file("config.json", data) {
    Ok(_) => {
        // File written successfully
    },
    Err(error) => {
        // Handle error
        println!("Failed to write file: {}", error);
    }
}
```

### Appending to Files

To append to a file:

```rust
let log_entry = format!("[{}] User logged in\n", get_timestamp());
match filesystem::append_file("logs/app.log", log_entry.into_bytes()) {
    Ok(_) => {
        // Log entry added successfully
    },
    Err(error) => {
        // Handle error
        println!("Failed to append to log: {}", error);
    }
}
```

## Directory Operations

### Creating Directories

To create a directory:

```rust
match filesystem::create_dir("data/uploads") {
    Ok(_) => {
        // Directory created successfully
    },
    Err(error) => {
        // Handle error
        println!("Failed to create directory: {}", error);
    }
}
```

### Listing Directory Contents

To list directory contents:

```rust
match filesystem::list_dir("data") {
    Ok(entries) => {
        for entry in entries {
            println!(
                "{}: {}",
                if entry.is_file { "FILE" } else { "DIR " },
                entry.name
            );
        }
    },
    Err(error) => {
        // Handle error
        println!("Failed to list directory: {}", error);
    }
}
```

## File Information

### Checking if a File Exists

To check if a file or directory exists:

```rust
match filesystem::exists("config.json") {
    Ok(exists) => {
        if exists {
            // File exists, proceed with operation
        } else {
            // File doesn't exist, handle accordingly
        }
    },
    Err(error) => {
        // Handle error
        println!("Failed to check file existence: {}", error);
    }
}
```

### Getting File Metadata

To get file metadata:

```rust
match filesystem::metadata("data/file.txt") {
    Ok(metadata) => {
        println!("Name: {}", metadata.name);
        println!("Size: {} bytes", metadata.size);
        println!("Is file: {}", metadata.is_file);
        println!("Is directory: {}", metadata.is_dir);
        
        if let Some(created) = metadata.created {
            println!("Created: {}", format_timestamp(created));
        }
        
        if let Some(modified) = metadata.modified {
            println!("Modified: {}", format_timestamp(modified));
        }
    },
    Err(error) => {
        // Handle error
        println!("Failed to get file metadata: {}", error);
    }
}
```

## Path Resolution

All paths in the Filesystem Handler are resolved relative to the base directory specified in the configuration. This provides a security boundary that prevents actors from accessing files outside their designated area.

For example, if the base directory is configured as `data/my-actor`:

```rust
// Actual path: data/my-actor/config.json
filesystem::read_file("config.json");

// Actual path: data/my-actor/logs/app.log
filesystem::write_file("logs/app.log", content);
```

Attempts to access files outside this boundary using path traversal (e.g., `../other-actor/file.txt`) will be blocked by the handler.

## State Chain Integration

All filesystem operations are recorded in the actor's state chain, creating a verifiable history. The chain events include:

1. **FilesystemOperation**: Records details of the operation:
   - Operation type (read, write, list, etc.)
   - Path
   - Size (for read/write operations)
   - Success/failure status

2. **Error**: Records any errors that occur:
   - Operation type
   - Path
   - Error message

This integration ensures that all file interactions are:
- Traceable
- Verifiable
- Reproducible
- Auditable

## Error Handling

The Filesystem Handler provides detailed error information for various failure scenarios:

1. **Permission Errors**: When trying to access files outside the allowed path
2. **Not Found Errors**: When a file or directory doesn't exist
3. **IO Errors**: When read/write operations fail
4. **Format Errors**: When paths are invalid
5. **Operation Not Allowed**: When trying to use a disabled operation

All errors are returned as strings and are also recorded in the state chain.

## Security Considerations

When using the Filesystem Handler, consider the following security aspects:

1. **Path Configuration**: Set the base path to limit file access
2. **Limited Permissions**: Use `allowed_commands` to restrict operations
3. **Input Validation**: Validate file paths before using them
4. **Content Validation**: Validate file contents before writing
5. **Error Handling**: Properly handle all error cases
6. **Resource Limits**: Be mindful of file sizes and disk usage

## Implementation Details

Under the hood, the Filesystem Handler:

1. Validates all paths against the configured base directory
2. Translates WIT interface calls to Rust's standard library file operations
3. Handles errors and security checks
4. Records all operations in the state chain
5. Manages file handles and resources properly

## Limitations

The current Filesystem Handler implementation has some limitations:

1. **No Streaming**: Large files are loaded fully into memory
2. **No Symbolic Link Following**: Symbolic links are not followed
3. **Limited Metadata**: Some platform-specific file metadata is not available
4. **No File Locking**: Concurrent access is not protected by file locks
5. **No Special Files**: Device files, sockets, etc. are not supported

## Best Practices

1. **Path Management**: Use relative paths within your base directory
2. **Error Handling**: Always handle file operation errors properly
3. **Resource Cleanup**: Clean up temporary files when they're no longer needed
4. **Directory Structure**: Create a clear directory structure for your actor's data
5. **Rate Limiting**: Implement rate limiting for frequent file operations
6. **Backups**: Implement backup mechanisms for important data

## Examples

### Example 1: Configuration Management

```rust
// Load configuration
fn load_config() -> Result<Config, String> {
    match filesystem::exists("config.json") {
        Ok(exists) => {
            if exists {
                match filesystem::read_file("config.json") {
                    Ok(content) => {
                        let config: Config = serde_json::from_slice(&content)
                            .map_err(|e| format!("Failed to parse config: {}", e))?;
                        Ok(config)
                    },
                    Err(e) => Err(format!("Failed to read config: {}", e)),
                }
            } else {
                // Create default config if not exists
                let default_config = Config::default();
                save_config(&default_config)?;
                Ok(default_config)
            }
        },
        Err(e) => Err(format!("Failed to check config existence: {}", e)),
    }
}

// Save configuration
fn save_config(config: &Config) -> Result<(), String> {
    let content = serde_json::to_vec(config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    
    filesystem::write_file("config.json", content)
        .map_err(|e| format!("Failed to write config: {}", e))
}
```

### Example 2: Log Management

```rust
fn log_event(event: &Event) -> Result<(), String> {
    // Create logs directory if not exists
    match filesystem::exists("logs") {
        Ok(exists) => {
            if !exists {
                filesystem::create_dir("logs")
                    .map_err(|e| format!("Failed to create logs directory: {}", e))?;
            }
        },
        Err(e) => return Err(format!("Failed to check logs directory: {}", e)),
    }
    
    // Format log entry
    let timestamp = chrono::Utc::now().to_rfc3339();
    let log_entry = format!("[{}] {}\n", timestamp, event.to_string());
    
    // Append to log file
    filesystem::append_file("logs/events.log", log_entry.into_bytes())
        .map_err(|e| format!("Failed to write log: {}", e))
}

// Rotate logs if they get too large
fn rotate_logs_if_needed() -> Result<(), String> {
    match filesystem::metadata("logs/events.log") {
        Ok(metadata) => {
            if metadata.size > MAX_LOG_SIZE {
                // Generate backup filename with timestamp
                let timestamp = chrono::Utc::now().timestamp();
                let backup_name = format!("logs/events-{}.log", timestamp);
                
                // Read current log
                let content = filesystem::read_file("logs/events.log")
                    .map_err(|e| format!("Failed to read log for rotation: {}", e))?;
                
                // Write to backup file
                filesystem::write_file(&backup_name, content)
                    .map_err(|e| format!("Failed to write backup log: {}", e))?;
                
                // Clear original log
                filesystem::write_file("logs/events.log", vec![])
                    .map_err(|e| format!("Failed to clear log: {}", e))?;
            }
            Ok(())
        },
        Err(_) => Ok(()), // Log file doesn't exist yet, nothing to rotate
    }
}
```

## Related Topics

- [Store Handler](store.md) - Alternative storage mechanism with content-addressable features
- [HTTP Framework Handler](http-framework.md) - For creating HTTP endpoints that serve files
- [Runtime Handler](runtime.md) - For accessing runtime information and operations

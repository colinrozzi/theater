# Theater Filesystem Handler

Provides filesystem access capabilities to WebAssembly actors in the Theater system.

## Features

This handler allows actors to:
- **Read files** - Read file contents as bytes
- **Write files** - Write string contents to files
- **List files** - List files in a directory
- **Delete files** - Remove individual files
- **Create directories** - Create new directories
- **Delete directories** - Remove directories and their contents
- **Check path existence** - Verify if a path exists
- **Execute commands** - Execute shell commands (with restrictions)
- **Execute nix commands** - Run nix development commands

All operations support permission-based access control with allowed/denied paths.

## Usage

Add this handler when creating your Theater runtime:

```rust
use theater_handler_filesystem::FilesystemHandler;
use theater::config::actor_manifest::FileSystemHandlerConfig;
use theater::config::permissions::FileSystemPermissions;
use std::path::PathBuf;

// Create handler configuration
let config = FileSystemHandlerConfig {
    path: Some(PathBuf::from("/workspace")),
    new_dir: Some(false),  // Set to true to create a temporary directory
    allowed_commands: Some(vec!["nix".to_string()]),  // Whitelist commands
};

// Optional: Configure permissions
let permissions = Some(FileSystemPermissions {
    allowed_paths: Some(vec!["/workspace".to_string()]),
    ..Default::default()
});

// Create the handler
let filesystem_handler = FilesystemHandler::new(config, permissions);

// Register with your handler registry
registry.register(filesystem_handler);
```

## WIT Interface

This handler implements the `theater:simple/filesystem` interface:

```wit
interface filesystem {
    // Read file contents
    read-file: func(path: string) -> result<list<u8>, string>
    
    // Write file contents
    write-file: func(path: string, contents: string) -> result<_, string>
    
    // List files in directory
    list-files: func(path: string) -> result<list<string>, string>
    
    // Delete a file
    delete-file: func(path: string) -> result<_, string>
    
    // Create a directory
    create-dir: func(path: string) -> result<_, string>
    
    // Delete a directory
    delete-dir: func(path: string) -> result<_, string>
    
    // Check if path exists
    path-exists: func(path: string) -> result<bool, string>
    
    // Execute command
    execute-command: func(dir: string, command: string, args: list<string>) 
        -> result<command-result, string>
    
    // Execute nix development command
    execute-nix-command: func(dir: string, command: string) 
        -> result<command-result, string>
}
```

## Configuration

### FileSystemHandlerConfig
- `path`: Optional base path for filesystem operations
- `new_dir`: If true, creates a random temporary directory under `/tmp/theater`
- `allowed_commands`: Optional whitelist of commands allowed for execution

### FileSystemPermissions
- `allowed_paths`: List of paths that actors are allowed to access
- Paths are validated and canonicalized to prevent directory traversal
- Both creation and access operations are checked against permissions

## Permission Validation

The handler includes comprehensive path validation:

1. **Creation operations** (write, create-dir):
   - Validates the parent directory exists and is allowed
   - Constructs the final path from validated parent + filename

2. **Access operations** (read, list, delete, path-exists):
   - Validates the target path exists and is allowed
   - Returns the canonicalized path

3. **Path canonicalization**:
   - Uses `dunce` library for robust Windows/Unix path handling
   - Resolves `.`, `..`, symlinks, etc.
   - Prevents directory traversal attacks

## Command Execution

Command execution is heavily restricted for security:

- Only `nix` commands are allowed
- Command arguments are validated against a whitelist
- Directory must be within allowed paths
- All execution is logged to the actor's chain

Currently allowed commands:
- `nix develop --command bash -c "cargo component build --target wasm32-unknown-unknown --release"`
- `nix flake init`

## Events

The handler records detailed events to the actor's chain:
- `filesystem-setup` - Handler initialization
- `theater:simple/filesystem/*` - All filesystem operations
- `permission-denied` - Permission violations with detailed reasons
- Command execution and results

## Architecture

The handler is split into logical modules for maintainability:

- `lib.rs` - Main handler implementation and Handler trait
- `types.rs` - Type definitions and error types
- `path_validation.rs` - Path resolution and permission checking
- `operations/basic_ops.rs` - File and directory operations
- `operations/commands.rs` - Command execution functionality

## Example

```rust
// Inside a WASM actor
use theater_simple::filesystem;

// Read a file
let contents = filesystem::read_file("data.txt")?;

// Write a file
filesystem::write_file("output.txt", "Hello, world!")?;

// List directory contents
let files = filesystem::list_files(".")?;

// Check if path exists
if filesystem::path_exists("config.json")? {
    // File exists
}
```

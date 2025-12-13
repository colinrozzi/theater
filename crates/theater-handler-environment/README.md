# theater-handler-environment

Environment variable handler for Theater WebAssembly actors.

## Overview

This handler provides read-only access to environment variables for WebAssembly actors running in the Theater runtime. It implements security controls through permission-based access and logs all environment variable operations to the actor's chain for debugging and auditing.

## Features

- **Read-only access**: Actors can read but not modify environment variables
- **Permission-based access**: Control which variables actors can access
- **Variable listing**: Optionally allow actors to list all accessible variables
- **Event logging**: All environment variable accesses are logged to the chain
- **Error handling**: Graceful handling of missing variables and permission denials

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
theater-handler-environment = "0.2.1"
```

### Basic Example

```rust
use theater_handler_environment::EnvironmentHandler;
use theater::config::actor_manifest::EnvironmentHandlerConfig;

// Create handler with default config
let config = EnvironmentHandlerConfig {
    allow_list_all: false, // Don't allow listing all variables
};
let handler = EnvironmentHandler::new(config, None);
```

### With Permissions

```rust
use theater_handler_environment::EnvironmentHandler;
use theater::config::actor_manifest::EnvironmentHandlerConfig;
use theater::config::permissions::EnvironmentPermissions;

// Create permissions allowing specific variables
let permissions = EnvironmentPermissions {
    allowed_variables: vec!["HOME".to_string(), "USER".to_string()],
    denied_variables: vec!["SECRET_KEY".to_string()],
};

let config = EnvironmentHandlerConfig {
    allow_list_all: false,
};

let handler = EnvironmentHandler::new(config, Some(permissions));
```

## WIT Interface

This handler implements the `theater:simple/environment` interface:

```wit
interface environment {
    // Get the value of an environment variable
    get-var: func(name: string) -> option<string>;
    
    // Check if an environment variable exists
    exists: func(name: string) -> bool;
    
    // List all accessible environment variables (if enabled)
    list-vars: func() -> list<tuple<string, string>>;
}
```

## Configuration

### EnvironmentHandlerConfig

- `allow_list_all`: Whether to allow the `list-vars` function (default: `false`)

### EnvironmentPermissions

- `allowed_variables`: List of specific variables that actors can access
- `denied_variables`: List of variables that actors cannot access (takes precedence)

If no permissions are provided, all variables are accessible.

## Chain Events

All environment operations are logged as chain events:

- `HandlerSetupStart`: Handler initialization begins
- `LinkerInstanceSuccess`: WASM linker setup successful
- `GetVar`: Environment variable access attempt
- `PermissionDenied`: Access denied due to permissions
- `HandlerSetupSuccess`: Handler setup completed

## Security Considerations

1. **Read-only**: This handler never allows modification of environment variables
2. **Permission checking**: All accesses are checked against configured permissions
3. **Logging**: All accesses are logged for auditing
4. **Fail-safe**: Permission denials return empty/false rather than errors

## Migration Notes

This handler was migrated from the core `theater` crate as part of the handler modularization effort. The migration included:

- Renamed from `EnvironmentHost` to `EnvironmentHandler`
- Implemented the `Handler` trait
- Made `setup_host_functions` synchronous (was async but never awaited)
- Added `Clone` derive for handler reusability
- Improved documentation

## License

Apache-2.0

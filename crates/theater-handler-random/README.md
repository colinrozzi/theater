# Theater Handler: Random

A random number generation handler for the Theater WebAssembly actor runtime.

## Overview

This handler provides secure random number generation capabilities to WebAssembly actors. It supports:

- **Random bytes generation** - Generate cryptographically secure random bytes
- **Random integers in ranges** - Generate random numbers within specified bounds
- **Random floats** - Generate random floating-point numbers between 0.0 and 1.0
- **UUID generation** - Generate v4 UUIDs

## Features

- **Reproducible randomness** - Optional seeding for deterministic behavior
- **Permission controls** - Fine-grained access control via permissions
- **Chain integration** - All random operations are logged to the actor's chain for debugging
- **Resource limits** - Configurable limits on byte generation and integer ranges

## Usage

### Adding to a Theater Runtime

```rust
use theater_handler_random::RandomHandler;
use theater::config::actor_manifest::RandomHandlerConfig;
use theater::handler::Handler;

// Create configuration
let config = RandomHandlerConfig {
    seed: None,                    // Use OS entropy (or Some(12345) for deterministic)
    max_bytes: 1024 * 1024,       // Maximum bytes per request
    max_int: u64::MAX,             // Maximum integer value
    allow_crypto_secure: true,     // Allow cryptographic operations
};

// Create the handler
let random_handler = RandomHandler::new(config, None);

// Register with the Theater runtime
registry.register(random_handler);
```

### Configuration Options

- **`seed`**: Optional u64 seed for reproducible random sequences
- **`max_bytes`**: Maximum number of bytes that can be requested in a single call
- **`max_int`**: Maximum integer value that can be generated
- **`allow_crypto_secure`**: Whether to allow cryptographic-quality randomness

### Permissions

You can restrict random operations using permissions:

```rust
use theater::config::permissions::RandomPermissions;

let permissions = RandomPermissions {
    allow_random_bytes: true,
    max_bytes_per_request: 1024,
    allow_random_range: true,
    allow_random_float: true,
    allow_uuid_generation: true,
};

let handler = RandomHandler::new(config, Some(permissions));
```

## WIT Interface

This handler implements the `theater:simple/random` interface:

```wit
interface random {
    random-bytes: func(size: u32) -> result<list<u8>, string>
    random-range: func(min: u64, max: u64) -> result<u64, string>
    random-float: func() -> result<float64, string>
    generate-uuid: func() -> result<string, string>
}
```

## Migration from Built-in Handler

This handler was migrated from the core Theater runtime to enable:

1. **Modularity** - Use only the handlers you need
2. **Independent versioning** - Update handlers without updating the runtime
3. **Easier maintenance** - Clear separation of concerns
4. **Third-party handlers** - Pattern for building custom handlers

## License

Apache-2.0

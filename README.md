# Theater

A WebAssembly actor system that enables state management, verification, and flexible interaction patterns. 
This project is in active development and not yet ready for production use. Most of the code is undocumented, untested, and subject to change.
If you are interested at all in the project or have questions, please reach out to me at colinrozzi@gmail.com.

[Read more about why we built Theater and its core concepts →](book/src/why-theater.md)

## Quick Start with Theater CLI

Theater includes a powerful CLI tool for managing the entire actor lifecycle:

```bash
# Create a new actor project
theater create my-actor

# Build the WebAssembly actor
cd my-actor
theater build

# Start a Theater server
theater server

# Start the actor
theater start manifest.toml

# List running actors
theater list

# View actor logs
theater logs <actor-id>

# Start actor and subscribe to its events in one command
theater start manifest.toml --id-only | theater subscribe -
```

[See complete CLI documentation →](book/src/cli.md)

## Features

- **Actor State Management**: Actors maintain verifiable state with complete history
- **Hash Chain Verification**: All state changes are recorded in a verifiable hash chain
- **Content-Addressable Storage**: Built-in store system for efficient data persistence and sharing
- **Secure Actor Identification**: Cryptographically secure UUID-based identification system
- **Multiple Interface Types**: Support for:
  - Actor-to-actor messaging
  - HTTP server capabilities
  - HTTP client capabilities
  - Parent-child supervision
  - Extensible interface system

## Documentation

- [Why Theater](book/src/why-theater.md) - Core concepts and motivation
- [CLI Documentation](book/src/cli.md) - Complete guide to the Theater CLI
- [Store System](book/src/store/README.md) - Content-addressable storage documentation
- [Making Changes](book/src/making-changes.md) - Guide for contributing changes
- [Current Changes](/changes/in-progress.md) - Overview of work in progress

# Supervision System

Theater provides a robust supervision system that enables parent actors to manage their children:

## Parent-Child Relationships

Actors can spawn and manage child actors using the supervisor interface:

```toml
# Parent actor manifest
name = "parent-actor"
component_path = "parent.wasm"

[[handlers]]
type = "supervisor"
config = {}
```

## Supervisor Interface

Parent actors can:
- Spawn new child actors
- List their children
- Monitor child status
- Stop and restart children
- Access child state and event history

Example usage in a parent actor:

```rust
use theater::supervisor::*;

// Spawn a child actor
let child_id = supervisor::spawn_child("child_manifest.toml")?;

// Get child status
let status = supervisor::get_child_status(&child_id)?;

// Restart child if needed
if status != ActorStatus::Running {
    supervisor::restart_child(&child_id)?;
}
```

## Development Setup

### Using Nix (Recommended)

Theater uses Nix with flakes for reproducible development environments. Here's how to get started:

1. First, install Nix:
https://nixos.org/download/

2. Enable flakes by either:
   - Adding to your `/etc/nix/nix.conf`:
     ```
     experimental-features = nix-command flakes
     ```
   - Or using the environment variable for each command:
     ```bash
     export NIX_CONFIG="experimental-features = nix-command flakes"
     ```

3. Clone the repository:
   ```bash
   git clone https://github.com/colinrozzi/theater.git
   cd theater
   ```

4. Enter the development environment:
   ```bash
   nix develop
   ```

The development environment provides:
- Rust toolchain with the exact version needed
- LLVM and Clang for wasmtime
- All required system dependencies
- Development tools like `cargo-watch`, `cargo-expand`, etc.

#### Common Nix Commands

- Start a new shell with the development environment:
  ```bash
  nix develop
  ```

- Run a command in the development environment without entering a shell:
  ```bash
  nix develop --command cargo build
  ```

- Update flake dependencies:
  ```bash
  nix flake update
  ```

#### Troubleshooting Nix

- If you see "experimental feature" errors:
  Make sure you've enabled flakes as described above.

- If you see "nix-command" errors:
  Your Nix installation might be too old. Update it with:
  ```bash
  nix-env -iA nixpkgs.nix
  ```

### Manual Setup

If you prefer not to use Nix, you'll need:

1. Rust 1.81.0 or newer
2. LLVM and Clang for building wasmtime
3. CMake
4. OpenSSL and pkg-config

Then:

1. Clone the repository:
```bash
git clone https://github.com/colinrozzi/theater.git
cd theater
```

2. Build the project:
```bash
cargo build
```

## Running

You can run Theater using either the CLI or directly with cargo:

```bash
# Using the Theater CLI (recommended)
theater server
theater start path/to/your/manifest.toml

# Or using cargo directly
cargo run -- --manifest path/to/your/manifest.toml
```

## Actor Manifests

Actors are configured using TOML manifests. Example:

```toml
name = "my-actor"
component_path = "path/to/actor.wasm"

[interface]
implements = "ntwk:simple-actor/actor"
requires = []

# Optional message-server capability
[[handlers]]
type = "message-server"
config = { port = 8080 }
interface = "ntwk:theater/message-server-client"

# Optional HTTP server capability
[[handlers]]
type = "http-server"
config = { port = 8081 }
```

Note: The message-server handler is optional and requires implementing the `message-server-client` interface if you want your actor to handle messages.

## Development Tools

When using the Nix development environment, you get access to several useful tools:

- `cargo clippy` - Run the Rust linter
- `cargo fmt` - Format your code
- `cargo test` - Run the test suite
- `cargo watch` - Watch for changes and automatically rebuild
- `cargo expand` - Show macro expansions
- `cargo udeps` - Find unused dependencies

## Contributing

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Run the tests (`cargo test`)
4. Run the linter (`cargo clippy`)
5. Format your code (`cargo fmt`)
6. Commit your changes (`git commit -m 'Add some amazing feature'`)
7. Push to the branch (`git push origin feature/amazing-feature`)
8. Open a Pull Request

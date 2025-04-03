# Theater

A WebAssembly actor system designed for the AI-generated code era.

## The Challenge

LLMs present incredible opportunities for software, but they also present significant challenges. It's realistic to assume that in the very near future, significantly more software will be written, and much of it may never see human review. Because of this, many assumptions we've made in building the software ecosystem are no longer valid.

## The Theater Approach

Theater is an attempt to move a lot of the trust from the code and its author to the system itself. Similar to how rust provides memory safety at the language level, Theater provides guarantees at the system level. If your application runs in Theater, you can be sure that it is sandboxed, deterministic, traceable, and fault-tolerant.

1. **WebAssembly Components** provide sandboxing and determinism
2. **Actor Model with Supervision** implements an Erlang-style supervision system for isolation and fault-tolerance
3. **Chain** tracks all information that enters or leaves the WebAssembly sandbox

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

## Key Features

- **Robust Supervision System**: Parent actors can spawn, monitor, and restart child actors
- **Complete State History**: All state changes are recorded in a verifiable chain
- **Content-Addressable Storage**: Built-in store system for efficient data persistence
- **Secure Actor Identification**: Cryptographically secure UUID-based identification
- **Multiple Interface Types**: Support for:
  - Actor-to-actor messaging
  - HTTP server capabilities
  - HTTP client capabilities
  - Parent-child supervision
  - Extensible interface system

## Documentation

- [Why Theater](docs/why-theater.md) - Core concepts and motivation
- [CLI Documentation](docs/cli.md) - Complete guide to the Theater CLI
- [Store System](docs/store/README.md) - Content-addressable storage documentation
- [Testing](book/docs/testing.md) - Testing strategy and infrastructure
- [Making Changes](docs/making-changes.md) - Guide for contributing changes
- [Current Changes](/changes/in-progress.md) - Overview of work in progress

## Supervision System

Theater provides a robust supervision system that enables parent actors to manage their children:

```toml
# Parent actor manifest
name = "parent-actor"
component_path = "parent.wasm"

[[handlers]]
type = "supervisor"
config = {}
```

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

## Contributing

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Run the tests (`cargo test`)
4. Run the linter (`cargo clippy`)
5. Format your code (`cargo fmt`)
6. Commit your changes (`git commit -m 'Add some amazing feature'`)
7. Push to the branch (`git push origin feature/amazing-feature`)
8. Open a Pull Request

## Status

This project is in active development and not yet ready for production use. Most of the code is undocumented, untested, and subject to change. If you are interested at all in the project or have questions, please reach out to me at colinrozzi@gmail.com.

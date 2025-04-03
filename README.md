# Theater

A WebAssembly actor system designed for the AI-generated code era.

## The Challenge

LLMs present incredible opportunities for software, but they also present significant challenges. It's realistic to assume that in the very near future, significantly more software will be written, and much of it may never see human review. Because of this, many assumptions we've made in building the software ecosystem are no longer valid.

## The Theater Approach

Theater is an attempt to move a lot of the trust from the code and its author to the system itself. Similar to how rust provides memory safety at the language level, Theater provides guarantees at the system level. If your application runs in Theater, you can be sure that it is sandboxed, deterministic, traceable, and fault-tolerant.

1. **WebAssembly Components** provide sandboxing and determinism
2. **Actor Model with Supervision** implements an Erlang-style supervision system for isolation and fault-tolerance
3. **Chain** tracks all information that enters or leaves the WebAssembly sandbox

## Status

This project is in active development and not yet ready for production use. Most of the code is undocumented, untested, and subject to change. If you are interested at all in the project or have questions, please reach out to me at colinrozzi@gmail.com.

## Quick Start with Theater CLI

Theater includes a powerful CLI tool for managing the entire actor lifecycle:

```bash
# Create a new actor project
theater create my-actor

# Build the WebAssembly actor
cd my-actor
theater build

# Start a Theater server in another terminal
theater server

# Start the actor
theater start manifest.toml

# List running actors
theater list

# View actor logs
theater events <actor-id>

# Start actor and subscribe to its events in one command
theater start manifest.toml --id-only | theater subscribe -
```

[See complete CLI documentation →](book/src/cli.md)

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


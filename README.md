# Theatre

A WebAssembly actor system that enables state management, verification, and flexible interaction patterns.

## Features

- **Actor State Management**: Actors maintain verifiable state with complete history
- **Hash Chain Verification**: All state changes are recorded in a verifiable hash chain
- **Multiple Interface Types**: Support for:
  - Actor-to-actor messaging
  - HTTP server capabilities
  - HTTP client capabilities
  - Extensible interface system

## Quick Start

1. Install Rust and cargo
2. Clone the repository:
```bash
git clone https://github.com/colinrozzi/theater.git
cd theater
```

3. Build the project:
```bash
cargo build
```

4. Run an actor with a manifest:
```bash
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

[[handlers]]
type = "Http"
config = { port = 8080 }

[[handlers]]
type = "Http-server"
config = { port = 8081 }
```


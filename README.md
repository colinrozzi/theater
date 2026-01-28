# Theater

A WebAssembly actor runtime for reproducible, isolated, and observable programs.

Theater runs WebAssembly components as actors with complete traceability - every interaction crossing the sandbox boundary is captured in an event chain. This enables debugging, replay, and verification of program execution.

## Key Features

- **Actor Model**: Erlang-style actors with supervision hierarchies for fault tolerance
- **Event Chain**: Every host call is recorded, enabling deterministic replay and debugging
- **WebAssembly Isolation**: Actors run in sandboxed WASM components with explicit capabilities
- **Pack Runtime**: Uses [Pack](https://github.com/colinrozzi/pack) for Graph ABI-based WASM execution
- **Handler System**: Modular capabilities (runtime, messaging, storage, supervision)

## Getting Started

### Prerequisites

- [Nix](https://nixos.org/download.html) with flakes enabled (recommended)
- Or: Rust 1.83.0+, with `wasm32-unknown-unknown` and `wasm32-wasip1` targets

### With Nix (recommended)

```bash
git clone https://github.com/colinrozzi/theater.git
cd theater
nix develop
cargo build
```

### Without Nix

```bash
git clone https://github.com/colinrozzi/theater.git
cd theater

# Install WASM targets
rustup target add wasm32-unknown-unknown wasm32-wasip1

# Build (requires Pack to be available at ../pack)
cargo build
```

## Creating an Actor

Use the CLI to scaffold a new actor:

```bash
# Create a new actor project
theater new my-actor

cd my-actor
theater build
theater run
```

Actors implement the `theater:simple/actor` interface:

```rust
use bindings::exports::theater::simple::actor::Guest;
use bindings::theater::simple::runtime::log;

struct Component;

impl Guest for Component {
    fn init(state: Option<Vec<u8>>, params: (String,)) -> Result<(Option<Vec<u8>>,), String> {
        log("Actor initialized!");
        Ok((state,))
    }
}
```

## Architecture

```
┌─────────────────────────────────────────────────┐
│                 TheaterRuntime                   │
│  ┌───────────────────────────────────────────┐  │
│  │              ActorRuntime                  │  │
│  │  ┌─────────────────────────────────────┐  │  │
│  │  │         PackInstance (WASM)         │  │  │
│  │  │                                     │  │  │
│  │  │   Actor Code + State                │  │  │
│  │  └─────────────────────────────────────┘  │  │
│  │                    │                       │  │
│  │              ┌─────┴─────┐                │  │
│  │              │  Handlers │                │  │
│  │              └───────────┘                │  │
│  │       runtime│message│store│supervisor    │  │
│  └───────────────────────────────────────────┘  │
│                       │                          │
│                 Event Chain                      │
└─────────────────────────────────────────────────┘
```

## Handlers

Handlers provide capabilities to actors:

| Handler | Description |
|---------|-------------|
| `runtime` | Logging, shutdown, event chain access |
| `message-server` | Actor-to-actor messaging |
| `store` | Content-addressed storage |
| `supervisor` | Spawn and manage child actors |

## Project Status

Theater is in active development. The API is stabilizing but breaking changes may occur.

## Contributing

Contributions welcome! Please open an issue to discuss significant changes before submitting PRs.

## License

Apache-2.0

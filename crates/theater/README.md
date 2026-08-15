# Theater

A WebAssembly actor runtime for reproducible, isolated, and observable programs.

Theater runs WebAssembly components as actors with complete traceability — every interaction crossing the sandbox boundary is captured in an event chain. This enables debugging, replay, and verification of program execution. It is built as infrastructure for AI-agent systems, moving trust from the individual agents to the system itself.

> [!NOTE]
> This project is in early development — breaking changes are expected and security is not yet guaranteed. Documentation is still filling in; if you're interested I'd love to hear from you at colinrozzi@gmail.com.

## Key Features

- **Actor Model**: Erlang-style actors with supervision hierarchies for fault tolerance
- **Event Chain**: Every host call is recorded, enabling deterministic replay and debugging
- **WebAssembly Isolation**: Actors run in sandboxed WASM components with explicit capabilities
- **Pack Runtime**: Uses [Pack](https://github.com/colinrozzi/pack) for Graph ABI-based WASM execution
- **Handler System**: Modular capabilities (runtime, messaging, storage, supervision)

## Getting Started

### Prerequisites

- [Nix](https://nixos.org/download.html) with flakes enabled (recommended), or
- Rust 1.83.0+ with the `wasm32-unknown-unknown` and `wasm32-wasip1` targets, plus LLVM/Clang, CMake, and OpenSSL + pkg-config (needed to build wasmtime)

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

## Documentation

- **[Guide](https://colinrozzi.github.io/theater/guide)** — a comprehensive guide to Theater
- **[Reference](https://colinrozzi.github.io/theater/api/theater)** — full rustdoc documentation

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

Contributions welcome! Please open an issue to discuss significant changes before submitting PRs. Before opening one, run the tests (`cargo test`), the linter (`cargo clippy`), and the formatter (`cargo fmt`).

## License

Apache-2.0

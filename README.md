# Theater

A WebAssembly actor system that enables state management, verification, and flexible interaction patterns.

[Read more about why we built Theater and its core concepts →](docs/why-theater.md)

## Features

- **Actor State Management**: Actors maintain verifiable state with complete history
- **Hash Chain Verification**: All state changes are recorded in a verifiable hash chain
- **Multiple Interface Types**: Support for:
  - Actor-to-actor messaging
  - HTTP server capabilities
  - HTTP client capabilities
  - Extensible interface system

## Development Setup

### Using Nix (Recommended)

The easiest way to get started is using Nix with flakes enabled:

1. [Install Nix](https://nixos.org/download.html)
2. Enable flakes by either:
   - Adding `experimental-features = nix-command flakes` to your `/etc/nix/nix.conf`
   - Or using `nix --experimental-features 'nix-command flakes'` for each command

3. Clone the repository:
```bash
git clone https://github.com/colinrozzi/theater.git
cd theater
```

4. Enter the development environment:
```bash
nix develop
```

This will set up everything you need including the correct Rust version and all required dependencies.

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

To run an actor with a manifest:
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

## License

[Insert your chosen license here]
# Building Theater Actors

The `theater build` command compiles Rust actors into WebAssembly components for use in the Theater system.

## Prerequisites

Before you can build Theater actors, you need:

1. **Rust and Cargo** - Install via [rustup](https://rustup.rs/)
2. **WebAssembly target** - Add with `rustup target add wasm32-unknown-unknown`
3. **cargo-component** - Install with `cargo install cargo-component`

## Building Your Actor

To build an actor:

```bash
# Navigate to your actor project directory
cd my-actor-project

# Build in release mode (default)
theater build

# Or build in debug mode
theater build --release false

# Clean build artifacts before building
theater build --clean
```

## Build Output

The build process will:

1. Compile your Rust project to a WebAssembly component
2. Place the compiled component at `target/wasm32-unknown-unknown/[debug|release]/<package_name>.wasm`
3. Update your manifest.toml (if it exists) with the path to the component

## Manifest

After building, you can:

- If you already have a manifest.toml, it will be updated with the new component path
- If you don't have a manifest.toml, you'll need to create one to deploy your actor:
  ```bash
  theater create-manifest --component-path target/wasm32-unknown-unknown/release/<package_name>.wasm
  ```

## Deployment

Once built, you can deploy your actor:

```bash
# If you have a manifest
theater start manifest.toml

# Or by specifying paths directly
theater start --component-path target/wasm32-unknown-unknown/release/<package_name>.wasm
```

## Troubleshooting

If you encounter errors:

- Make sure cargo-component is installed
- Verify you have the wasm32-unknown-unknown target installed
- Check for Rust compilation errors in your project
- Use the `--verbose` flag for more detailed output

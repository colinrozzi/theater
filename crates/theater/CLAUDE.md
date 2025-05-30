# Theater Development Guide

## Build Commands
- Build: `cargo build`
- Run: `cargo run -- --manifest path/to/manifest.toml`
- Format: `cargo fmt`
- Lint: `cargo clippy`
- Tests: `cargo test`
- Run single test: `cargo test test_name`
- Run tests with logs: `RUST_LOG=debug cargo test -- --nocapture`
- Watch mode: `cargo watch -x check -x test`

## Code Style Guidelines
- Use `anyhow` for general error handling, `thiserror` for custom errors
- Add context with `.context()` or `.with_context()`
- Group imports: std first, then external crates, then internal modules
- Use descriptive CamelCase for types, snake_case for functions
- Document public interfaces with doc comments (`///`)
- Tests in `#[cfg(test)]` modules with descriptive names
- Follow structured change process in `/changes/proposals/`
- Document decisions in working notes when implementing changes
- Use the TheaterId for entity identification
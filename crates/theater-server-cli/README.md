# Theater Server CLI

A standalone command-line tool for starting and managing Theater WebAssembly actor system servers.

## Installation

Build from the Theater workspace:

```bash
cargo build --bin theater-server
```

The binary will be available at `target/debug/theater-server` (or `target/release/theater-server` for release builds).

## Usage

### Start Server

```bash
theater-server [OPTIONS]
```

### Options

- `-a, --address <ADDRESS>` - Address to bind the theater server to (default: 127.0.0.1:9000)
- `-l, --log-level <LEVEL>` - Logging level: trace, debug, info, warn, error (default: info)
- `--log-filter <FILTER>` - Advanced logging filter (e.g. "theater=debug,wasmtime=info")
- `--log-dir <DIR>` - Log directory (default: $THEATER_HOME/logs/theater)
- `--log-stdout` - Also log to stdout
- `-h, --help` - Print help information
- `-V, --version` - Print version information

### Examples

Start server on default address:
```bash
theater-server
```

Start server on custom address with debug logging:
```bash
theater-server --address 0.0.0.0:8080 --log-level debug --log-stdout
```

Use advanced logging filter:
```bash
theater-server --log-filter "theater=debug,wasmtime=info,hyper=warn"
```

## Environment Variables

- `THEATER_HOME` - Base directory for Theater data and logs (used in default log-dir path)

## Migration from `theater server`

This tool replaces the `theater server` command from the main Theater CLI. The arguments and functionality remain identical:

**Old:**
```bash
theater server --address 127.0.0.1:9000 --log-level info
```

**New:**
```bash
theater-server --address 127.0.0.1:9000 --log-level info
```

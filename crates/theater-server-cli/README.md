# theater-server-cli

Standalone CLI for Theater server management.

## Overview

`theater-server-cli` provides a dedicated command-line interface for running and managing Theater servers. This is a lightweight alternative to the full `theater-cli` when you only need server functionality.

## Installation

```bash
cargo install theater-server-cli
```

## Usage

```bash
# Start a Theater server
theater-server start

# Start with custom configuration
theater-server start --config server.toml

# Show server status
theater-server status
```

## Configuration

The server can be configured via TOML files:

```toml
[server]
host = "127.0.0.1"
port = 8080
log_level = "info"

[security]
enable_cors = true
max_actors = 100
```

For complete documentation, see the [Theater Guide](https://colinrozzi.github.io/theater/guide).

## License

This project is licensed under the Apache License 2.0 - see the [LICENSE](../../LICENSE) file for details.

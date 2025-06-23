# theater-cli

Command-line interface for the Theater WebAssembly actor system.

## Installation

```bash
cargo install theater-cli
```

## Usage

The `theater` CLI provides a complete interface for managing Theater actors:

```bash
# Create a new agent project
theater create my-agent

# Build the WebAssembly agent
cd my-agent
theater build

# Start a Theater server
theater server

# Start an agent
theater start manifest.toml

# List running agents
theater list

# View agent events
theater events <agent-id>
```

For complete documentation, see the [Theater Guide](https://colinrozzi.github.io/theater/guide).

## Features

- Complete agent lifecycle management
- Interactive terminal UI for monitoring
- Server management and configuration
- Real-time event streaming and analysis
- Agent development workflows

## License

This project is licensed under the Apache License 2.0 - see the [LICENSE](../../LICENSE) file for details.

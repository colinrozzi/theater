# Theater CLI

The Theater CLI provides a comprehensive set of tools to manage WebAssembly actors and the Theater runtime.

## Core Features

- **Actor Management**: Start, stop, inspect, and interact with actors
- **Manifest Management**: Create, validate, and manage actor manifests
- **System Management**: Monitor system status, configure settings, backup and restore
- **Developer Utilities**: Scaffold projects, build components, test actors locally

## Command Structure

```
theater [OPTIONS] <COMMAND>

OPTIONS:
    -v, --verbose    Enable verbose output

COMMANDS:
    # Actor Management
    actor start      Start a new actor
    actor stop       Stop an actor
    actor list       List all running actors
    actor inspect    Show detailed information about a running actor
    actor restart    Restart an actor
    actor logs       View actor logs
    actor state      View/export actor state
    actor call       Send a message to an actor
    actor subscribe  Subscribe to actor events

    # Manifest Management
    manifest create    Generate a new actor manifest interactively
    manifest validate  Validate a manifest file
    manifest list      List available manifest templates

    # System Management
    system status    Show system status (running actors, resource usage)
    system config    View/edit system configuration
    system backup    Backup actor states
    system restore   Restore from backup

    # Development Utilities
    dev scaffold    Create a new actor project with templates
    dev build       Build a WASM component from source
    dev test        Test an actor locally
    dev watch       Watch for changes and auto-restart actors

    # Legacy Commands (Deprecated)
    start           Start a new actor (use 'actor start' instead)
    stop            Stop an actor (use 'actor stop' instead)
    list            List all running actors (use 'actor list' instead)
    subscribe       Subscribe to actor events (use 'actor subscribe' instead)
    interactive     Interactive mode (use specific commands instead)

    help            Print this message or the help of the given subcommand(s)
```

## Examples

### Starting an Actor

```bash
# Start an actor from a manifest
theater actor start my-actor.toml

# Start with a specific server address
theater actor start --address 192.168.1.100:9000 my-actor.toml
```

### Creating a Manifest

```bash
# Create a manifest interactively
theater manifest create --name my-http-actor

# Use a template
theater manifest create --template templates/http-server.toml
```

### Monitoring the System

```bash
# Show basic system status
theater system status

# Show detailed metrics and continuously update
theater system status --detailed --watch
```

### Development Workflow

```bash
# Scaffold a new project
theater dev scaffold --name my-new-actor --language rust --actor-type http-server

# Build a project
theater dev build --path ./my-new-actor --release

# Watch for changes and auto-restart
theater dev watch my-new-actor.toml
```

## Manifest Templates

The CLI includes several built-in templates for common actor types:

- `http-server.toml`: HTTP server actor
- `message-handler.toml`: Message processing actor
- `supervisor.toml`: Actor for managing child actors

List available templates:

```bash
theater manifest list --detailed
```

## Environment Variables

- `THEATER_SERVER`: Default address of the Theater server (default: 127.0.0.1:9000)
- `THEATER_LOG_LEVEL`: Default logging level (default: info)

## Configuration

The CLI can be configured via the following methods (in order of precedence):

1. Command-line arguments
2. Environment variables
3. Configuration file (~/.theater/config.toml)

## Server Protocol Extensions

Note: Many advanced features require corresponding server-side support. If you see "This feature requires server-side support" messages, it means the CLI is ready for these features, but the Theater server implementation needs to be extended to support them.

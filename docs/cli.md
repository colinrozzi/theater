# Theater CLI

The Theater CLI is a command-line tool that provides a convenient interface for working with the Theater WebAssembly actor system. It simplifies actor development, deployment, and management.

## Installation

The Theater CLI is included when you build the Theater project:

```bash
# Build the project
cargo build --release

# Use the CLI directly
./target/release/theater --help

# Or add it to your PATH for easier access
```

## Basic Usage

```bash
# Get help
theater --help

# Run commands with verbose output
theater -v <command>

# Get output in JSON format (for scripting)
theater --json <command>
```

## Command Overview

| Command   | Description                               |
|-----------|-------------------------------------------|
| `build`   | Build a Theater actor to WebAssembly      |
| `create`  | Create a new Theater actor project        |
| `deploy`  | Deploy an actor to a Theater server       |
| `events`  | Get actor events                          |
| `list`    | List all running actors                   |
| `logs`    | View actor logs                           |
| `message` | Send a message to an actor                |
| `restart` | Restart a running actor                   |
| `server`  | Start a Theater server                    |
| `start`   | Start an actor from a manifest            |
| `state`   | Get actor state                           |
| `stop`    | Stop a running actor                      |
| `watch`   | Watch a directory and redeploy on changes |

## Detailed Command Usage

### Creating a New Actor Project

```bash
# Create a basic actor project
theater create my-actor

# Create an HTTP actor project
theater create my-http-actor --template http

# Create a project in a specific directory
theater create my-actor --output-dir ~/projects
```

### Building a Theater Actor

```bash
# Build the actor in the current directory
theater build

# Build a specific project
theater build /path/to/project

# Build in debug mode
theater build --release false

# Clean and rebuild
theater build --clean
```

### Managing a Theater Server

```bash
# Start a server with default settings
theater server

# Start a server with a custom port
theater server --port 9001

# Start a server with a custom data directory
theater server --data-dir /path/to/data
```

### Deploying and Running Actors

```bash
# Deploy an actor using its manifest
theater deploy path/to/manifest.toml

# Start an actor from a manifest
theater start path/to/manifest.toml

# List all running actors
theater list

# View actor logs
theater logs <actor-id>

# Get actor state
theater state <actor-id>

# Get actor events
theater events <actor-id>

# Stop an actor
theater stop <actor-id>

# Restart an actor
theater restart <actor-id>
```

### Development Workflow

```bash
# Create a new actor project
theater create my-actor

# Build the actor
cd my-actor
theater build

# Deploy the actor
theater deploy manifest.toml

# Watch the directory and redeploy on changes
theater watch . --manifest manifest.toml
```

### Sending Messages to Actors

```bash
# Send a JSON message to an actor
theater message <actor-id> --data '{"action": "doSomething", "value": 42}'

# Send a message from a file
theater message <actor-id> --file message.json
```

## Output Formats

The Theater CLI supports human-readable output (default) and JSON output for scripting:

```bash
# Human-readable output
theater list

# JSON output
theater --json list
```

## Environment Variables

The Theater CLI respects the following environment variables:

- `THEATER_SERVER_ADDRESS`: Default server address (host:port)
- `THEATER_DATA_DIR`: Default data directory location

## Common Workflows

### Develop, Build, and Deploy Loop

1. Create a new actor project
   ```bash
   theater create my-actor
   cd my-actor
   ```

2. Build the actor
   ```bash
   theater build
   ```

3. Start a Theater server (in another terminal)
   ```bash
   theater server
   ```

4. Deploy the actor
   ```bash
   theater deploy manifest.toml
   ```

5. Watch for changes and automatically redeploy
   ```bash
   theater watch . --manifest manifest.toml
   ```

### Monitoring and Debugging

To monitor and debug actors:

1. List all running actors
   ```bash
   theater list
   ```

2. View actor logs
   ```bash
   theater logs <actor-id>
   ```

3. Inspect actor state
   ```bash
   theater state <actor-id>
   ```

4. View actor events
   ```bash
   theater events <actor-id>
   ```

5. Restart an actor if issues occur
   ```bash
   theater restart <actor-id>
   ```

## Advanced Usage

### HTTP Actor Setup

For HTTP actors:

1. Create an HTTP actor project
   ```bash
   theater create my-http-actor --template http
   ```

2. Build and deploy
   ```bash
   cd my-http-actor
   theater build
   theater deploy manifest.toml
   ```

3. The HTTP server will be available at the port specified in the manifest

### Supervisor Pattern

For parent-child actor relationships:

1. Create parent and child actors
2. Configure the parent with supervisor capabilities
3. Deploy the parent actor
4. The parent can then spawn and manage child actors

## Tips and Tricks

- Use the `--verbose` flag for detailed output during commands
- Use the `--json` flag to get structured output for scripting
- For faster development, use the `watch` command for automatic redeployment
- Check `theater --help` and `theater <command> --help` for specific command options

# Theater

A WebAssembly actor system designed for secure, transparent, and reliable AI agent infrastructure.

## The Challenge

AI agents present incredible opportunities for automation and intelligence, but they also introduce significant challenges. As autonomous AI agents become more capable and ubiquitous, we need infrastructure that can:

1. **Contain and secure** agents with precise permission boundaries
2. **Trace and verify** all agent actions for auditability and debugging
3. **Manage failures** gracefully in complex agent systems
4. **Orchestrate cooperation** between specialized agents with different capabilities

## The Theater Approach

Theater provides infrastructure specifically designed for AI agent systems. It moves trust from the individual agents to the system itself, providing guarantees at the infrastructure level:

1. **WebAssembly Components** provide sandboxing and deterministic execution, ensuring agents can only access explicitly granted capabilities
2. **Actor Model with Supervision** implements an Erlang-style supervision hierarchy for fault-tolerance and structured agent cooperation
3. **Event Chain** captures every agent action in a verifiable record, enabling complete traceability and deterministic replay

### Read more

- **[Guide](https://colinrozzi.github.io/theater/guide)** - A comprehensive guide to Theater
- **[Reference](https://colinrozzi.github.io/theater/api/theater)** - Full rustdoc documentation

## Quick Start with Theater CLI

Theater includes a powerful CLI tool for managing the entire agent lifecycle:

```bash
# Create a new agent project
theater create my-agent

# Build the WebAssembly agent
cd my-agent
theater build

# Start a Theater server in another terminal
theater server

# Start the agent
theater start manifest.toml

# List running agents
theater list

# View agent logs
theater events <agent-id>

# Start agent and subscribe to its events in one command
theater start manifest.toml --id-only | theater subscribe -
```

[See complete CLI documentation →](book/src/cli.md)

## Agent System Architecture

Theater enables sophisticated agent architectures through its actor model:

- **Agent Hierarchy**: Structure agents in parent-child relationships where parent agents can spawn, monitor, and control child agents
- **Specialized Agents**: Create purpose-built agents with specific capabilities and knowledge domains
- **Secure Communication**: Enable agent-to-agent communication through explicit message passing
- **Capability Controls**: Grant agents precisely the capabilities they need through configurable handlers

## Agent Supervision

Theater's supervision system enables robust agent management:

```toml
# Parent agent manifest
name = "supervisor-agent"
component_path = "supervisor.wasm"

[[handlers]]
type = "supervisor"
config = {}
```

Supervisor agents can:
- Spawn new specialized agents
- Monitor agent status and performance
- Restart failed agents automatically
- Access agent state and event history

Example usage in a supervisor agent:

```rust
use theater::supervisor::*;

// Spawn a specialized agent
let agent_id = supervisor::spawn_child("researcher_agent.toml")?;

// Get agent status
let status = supervisor::get_child_status(&agent_id)?;

// Restart agent if needed
if status != ActorStatus::Running {
    supervisor::restart_child(&agent_id)?;
}
```

## Complete Traceability

Theater captures every action agents take in a verifiable event chain:

- All messages between agents are recorded
- Every external API call is logged with inputs and outputs
- State changes are tracked with causal relationships
- The entire system can be deterministically replayed for debugging

## Development Setup

### Using Nix (Recommended)

Theater uses Nix with flakes for reproducible development environments. Here's how to get started:

1. First, install Nix:
   https://nixos.org/download/

2. Enable flakes by either:
   - Adding to your `/etc/nix/nix.conf`:
     ```
     experimental-features = nix-command flakes
     ```
   - Or using the environment variable for each command:
     ```bash
     export NIX_CONFIG="experimental-features = nix-command flakes"
     ```

3. Clone the repository:
   ```bash
   git clone https://github.com/colinrozzi/theater.git
   cd theater
   ```

4. Enter the development environment:
   ```bash
   nix develop
   ```

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

## Contributing

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Run the tests (`cargo test`)
4. Run the linter (`cargo clippy`)
5. Format your code (`cargo fmt`)
6. Commit your changes (`git commit -m 'Add some amazing feature'`)
7. Push to the branch (`git push origin feature/amazing-feature`)
8. Open a Pull Request

## Status

This project is in active development. Most features are focused on providing infrastructure for AI agent systems with security, traceability, and reliability as primary goals. If you are interested in the project or have questions, please reach out to me at colinrozzi@gmail.com.

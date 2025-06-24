# Theater

A WebAssembly actor system designed for secure, transparent, and reliable AI agent infrastructure.

> [!NOTE]
> This documentation is incomplete, please reach out to me at colinrozzi@gmail.com. I very much appreciate your interest and would love to hear from you! This project is in early development, breaking changes are expected, security is not guarenteed

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

### To get started:
1. Download [theater-server-cli](crates/theater-server-cli) 
   ```bash
   cargo install theater-server-cli
   ```
2. Download [theater-cli](crates/theater-cli) 
   ```bash
   cargo install theater-cli
   ```
3. Start the Theater server and leave it running in the background
   ```bash
   theater-server --log-stdout
   ```
4. Start an actor in a new terminal (this requires an actor, I will update this shortly with a link to a sample actor)
   ```bash
   theater start manifest.toml
   ```
5. Look at the running actors
   ```bash
   theater list
   ```
6. Look at an actor's event chain
   ```bash
   theater events <actor-id>
   ```
7. Stop an actor
   ```bash
   theater stop <actor-id>
   ```


### Manual Setup

Most people will have these things, move on to cloning and download the dependencies as you need, but just for clarity you will need:

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

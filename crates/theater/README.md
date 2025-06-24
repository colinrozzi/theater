# Theater

A WebAssembly actor system designed for secure, transparent, and reliable AI agent infrastructure.

> [!NOTE]
> This documentation is incomplete, please reach out to me at colinrozzi@gmail.com I very much appreciate your interest and would love to hear from you! This project is in early development, breaking changes are expected, security is not guarenteed

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

### Manual Setup

You'll need:

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
   - super broken rn don't worry about it, its on the list
5. Format your code (`cargo fmt`)
   - also on the list
6. Commit your changes (`git commit -m 'Add some amazing feature'`)
7. Push to the branch (`git push origin feature/amazing-feature`)
8. Open a Pull Request

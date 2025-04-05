# Theater Architecture

This section provides a detailed technical overview of the Theater system architecture, explaining how its components work together to create a secure, reliable runtime for WebAssembly actors.

Theater is built with a layered approach where components have clear responsibilities and well-defined interfaces. This design enables strong isolation boundaries between actors while maintaining high performance and reliability.

## In This Section

- [Component Overview](component-overview.md) - Detailed explanation of Theater's core components and their relationships
- [Runtime Implementation](runtime-implementation.md) - How the Theater runtime manages actor lifecycle and communication
- [WebAssembly Integration](wasm-integration.md) - Details of the WebAssembly component model implementation
- [Security Model](security-model.md) - How Theater enforces isolation and secure execution

Understanding the architecture is helpful for those who want to contribute to Theater's development or need to integrate Theater with other systems. However, if you're just getting started, we recommend beginning with the [Core Concepts](../core-concepts/README.md) section first.

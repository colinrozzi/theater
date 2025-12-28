# Theater Documentation

Welcome to the Theater documentation!

## Guides

### Handler Development

- **[Implementing WASI Handlers](./implementing-wasi-handlers.md)** - Complete guide to implementing WASI-compliant handlers in Theater, with a step-by-step walkthrough of the wasi:random implementation.

## Quick Links

- [Project README](../README.md)
- [Theater Crate](../crates/theater/)
- [Handler Implementations](../crates/theater-handler-random/)
- [Test Actors](../test-actors/)

## Overview

Theater is a WebAssembly Actor Runtime for building distributed systems. Key concepts:

- **Actors**: WebAssembly components that process messages
- **Handlers**: Capabilities that actors can import (filesystem, HTTP, random, etc.)
- **Event Chain**: Audit log of all operations for correctness and replay
- **WASI Compliance**: Standard WebAssembly System Interface support

## Contributing

When implementing new handlers or features:

1. Follow the WASI specification when implementing standard interfaces
2. Ensure all boundary-crossing data is logged to the event chain
3. Create test actors to validate functionality
4. Document your implementation following the existing guide structure

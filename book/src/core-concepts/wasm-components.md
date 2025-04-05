# WebAssembly Components & Sandboxing

WebAssembly Components form the foundation of Theater's security and determinism guarantees. This pillar explains how Theater uses WebAssembly to create isolated, predictable execution environments.

## What are WebAssembly Components?

WebAssembly Components are a standards-based approach to packaging, distributing, and executing code in a sandboxed environment. They build on core WebAssembly to provide:

- **Interface Types**: Rich data types that can be passed between components
- **Component Model**: A packaging format that defines interfaces and dependencies
- **WIT (WebAssembly Interface Type) Language**: A language for describing component interfaces

In Theater, each actor is implemented as a WebAssembly Component with well-defined interfaces.

## Security Boundaries

WebAssembly provides strong security guarantees:

- **Memory Isolation**: Each component has its own memory space
- **Capability-Based Security**: Components only get access to capabilities explicitly granted
- **No Native System Access**: Components cannot access host resources without permission

This means actors in Theater cannot:
- Access memory of other actors
- Access files or network unless explicitly permitted
- Execute arbitrary system commands

## Deterministic Execution

WebAssembly's design ensures predictable execution:

- **Well-Defined Semantics**: Behavior is clearly defined by the standard
- **No Undefined Behavior**: Unlike native code, there are no "undefined behavior" cases
- **Controlled Environment**: All external inputs are mediated by the host

This determinism is critical for Theater's event chain system, as it guarantees that replaying the same inputs will produce the same outputs.

## Language-Agnostic Components

One of the key benefits of WebAssembly Components is language neutrality:

- **Multiple Source Languages**: Write actors in Rust, AssemblyScript, C, C++, or other languages
- **Consistent Runtime Behavior**: Regardless of source language, runtime behavior is consistent
- **Interoperability**: Components written in different languages can communicate seamlessly

## Interface Definitions

Theater uses the WebAssembly Interface Type (WIT) language to define:

- **Actor Interfaces**: What functions an actor exposes
- **Host Functions**: What capabilities the system provides to actors
- **Message Formats**: The structure of data exchanged between components

These interface definitions create a contract between actors and the system, making it clear what each component can and cannot do.

## Benefits for Theater

By building on WebAssembly Components, Theater gains:

1. **Strong Isolation**: Actors cannot interfere with each other
2. **Security**: Limited capabilities reduce attack surface
3. **Determinism**: Predictable execution enables verification
4. **Portability**: Run the same actors across different platforms
5. **Language Choice**: Implement actors in your preferred language

These properties make Theater particularly well-suited for running untrusted or AI-generated code, as the system can provide strong guarantees about what that code can and cannot do.

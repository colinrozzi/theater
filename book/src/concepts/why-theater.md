# Why Theater?

## The Challenge of Trust in Modern Software

Software has always been built on a foundation of trust. We trust programmers to write code that works correctly, we trust organizations to review that code, and we trust the ecosystem to catch and fix bugs. This trust model has worked reasonably well, but it's about to be severely tested.

As Large Language Models (LLMs) begin to generate more and more code, we're entering an era where much of the software running in production may never have been reviewed by human eyes. The programmer—that essential link in the chain of trust—is increasingly being replaced by an AI system with different strengths and weaknesses than a human developer.

This shift raises several critical challenges:

1. **Quality Validation**: How do we validate the correctness of code when there's simply too much of it to review?
2. **Failure Containment**: When AI-generated code fails, how do we prevent it from taking down entire systems?
3. **Debugging Complexity**: How do we debug issues in code we didn't write and may not fully understand?
4. **Security Boundaries**: How do we ensure that untrusted code can't access or manipulate data it shouldn't?

These challenges aren't merely theoretical. They represent real problems that organizations will face as they integrate more AI-generated code into their workflows.

## The Theater Solution

Theater addresses these challenges by building guarantees into the structure of the software system itself, rather than relying on trust in the code or its author.

### 1. WebAssembly Sandboxing

Theater uses WebAssembly Components as its foundation, providing several critical benefits:

- **Strong Isolation**: Each actor runs in its own sandbox, preventing direct access to the host system or other actors.
- **Deterministic Execution**: WebAssembly's deterministic nature ensures that the same inputs always produce the same outputs, making systems easier to test and debug.
- **Language Agnosticism**: While Theater is built in Rust, actors can be written in any language that compiles to WebAssembly, expanding the ecosystem.

### 2. Erlang-Inspired Supervision

Taking inspiration from Erlang/OTP, Theater implements a comprehensive supervision system:

- **Hierarchical Recovery**: Parent actors can monitor their children and restart them if they fail, containing failures to small parts of the system.
- **Explicit Error Handling**: The supervision system makes error handling a first-class concern, not an afterthought.
- **Lifetime Isolation**: When an actor fails, its state and chain are preserved, allowing for anything from detailed debugging to the resumption of the actor with a previous good state.

### 3. Complete Traceability

Theater tracks all information that enters or leaves the WebAssembly sandbox:

- **State History**: Every state change is recorded and linked to its parent state, creating a complete and verifiable history of state transitions.
- **Deterministic Replay**: Because WebAssembly is sandboxed and deterministic, any sequence of events can be replayed exactly on any system.
- **Verification**: Each state transition is cryptographically linked to its predecessors, so state received from an external source can be verified.

By providing a structured environment with strong guarantees, Theater enables developers to build more trustworthy systems, even when the components themselves might not be fully trusted.

In the following chapters, we'll explore how to use Theater in practice, diving into the details of its architecture and showing how it can be applied to real-world problems.

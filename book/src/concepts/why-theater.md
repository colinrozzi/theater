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

Taking inspiration from Erlang/OTP, one of the most reliable systems ever built, Theater implements a comprehensive supervision system:

- **Hierarchical Recovery**: Parent actors can monitor their children and restart them if they fail, containing failures to small parts of the system.
- **Explicit Error Handling**: The supervision system makes error handling a first-class concern, not an afterthought.
- **Separation of Concerns**: Each actor has a single responsibility, making the system easier to reason about and maintain.

### 3. Complete Traceability

Theater tracks all information that enters or leaves the WebAssembly sandbox:

- **State History**: Every state change is recorded in a verifiable chain, creating a complete history of an actor's execution.
- **Time-Travel Debugging**: The state history allows developers to "go back in time" to understand how a system reached a particular state.
- **Audit Trail**: The comprehensive tracking creates an audit trail that can be invaluable for security and compliance.

## Beyond the Technical: A Philosophy of Safe Innovation

At its core, Theater represents a philosophy about how we should build software in an era of rapid innovation and increasing automation:

1. **Don't Trust, Verify**: Rather than trusting code or its author, verify its behavior at runtime.
2. **Fail Gracefully**: Design systems to handle failures as a normal part of operation, not an exceptional event.
3. **Make Complexity Transparent**: Build tools that make complex systems more understandable and debuggable.
4. **Enable Safe Experimentation**: Create environments where new approaches can be tried with minimal risk.

This philosophy acknowledges that the pace of software development is accelerating, and we need new tools and approaches to keep up while maintaining reliability and security.

## Where Theater Fits

Theater isn't meant to replace all software systems—it's designed to address specific challenges in a changing landscape:

- **AI Integration**: Systems that incorporate AI-generated components
- **Critical Infrastructure**: Applications where reliability and fault tolerance are paramount
- **Complex Microservices**: Distributed systems that require strong boundaries and clear communication patterns
- **Educational Environments**: Places where people are learning about distributed systems and actor models

By providing a structured environment with strong guarantees, Theater enables developers to build more trustworthy systems, even when the components themselves might not be fully trusted.

In the following chapters, we'll explore how to use Theater in practice, diving into the details of its architecture and showing how it can be applied to real-world problems.
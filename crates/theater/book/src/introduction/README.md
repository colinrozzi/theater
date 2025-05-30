# Overview

Theater is a WebAssembly actor system designed for a world where code may never see human review. It provides an environment where components can interact safely, failures can be contained, and the entire system can be traced and debugged with unprecedented clarity.

## A New Era of Software Development

We stand at the beginning of a transformation in how software is written. Large Language Models are already generating significant amounts of code, and this trend will only accelerate. Soon, a substantial portion of the code touching users may never have been seen by human eyes.

This shift presents both opportunities and challenges. On one hand, our software can become more adaptable and flexible, and allow us to tackle problems that were previously too complex or time-consuming. On the other hand, the fundamental assumptions that our software ecosystem is built upon—human review, intentional design, and careful testing—are being upended.

## The Three Pillars of Theater

Theater builds trust into the structure of the software system itself through three foundational pillars:

1. **WebAssembly Components & Sandboxing** provide security boundaries and deterministic execution, ensuring that code operates within well-defined constraints.

2. **Actor Model & Supervision** implements an Erlang-style actor system with hierarchical supervision, creating isolation between components and facilitating recovery from failures.

3. **Traceability & Verification** tracks all information that enters or leaves the WebAssembly sandbox, creating a comprehensive audit trail for debugging and verification.

These three pillars work together to create a system that is secure, resilient, and transparent, addressing the unique challenges of running AI-generated code at scale.

## Documentation Structure

This book is organized to provide a clear learning path:

- **Introduction**: Why Theater exists and the problems it solves
- **Core Concepts**: What Theater is and its fundamental principles
- **Architecture**: How Theater works internally
- **User Guide**: Practical information for using Theater
- **Development**: Building and extending Theater components
- **Services**: Built-in capabilities and handler systems

Each section builds on the previous ones, providing a progressively deeper understanding of the Theater system.

## Who Is Theater For?

Theater is currently an experimental project for:
- Developers exploring new approaches to software reliability
- Researchers interested in secure execution of untrusted code
- Early adopters ready to help shape the future of AI-aware systems

It is not intended for production use at this time but provides a glimpse into a future where systems are designed with AI-generated code in mind.

## About This Book

This book serves as a friendly introduction to the Theater system. It is meant for programmers new to Theater with existing programming experience. For a more in-depth and precise understanding of the system and its component parts, please refer to the [API Reference](/theater/api/theater/index.html).

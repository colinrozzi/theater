# Overview

Theater is a WebAssembly actor system designed for a world where code may never see human review. It provides an environment where components can interact safely, failures can be contained, and the entire system can be traced and debugged with unprecedented clarity.

## A New Era of Software Development

We stand at the beginning of a transformation in how software is written. Large Language Models are already generating significant amounts of code, and this trend will only accelerate. Soon, a substantial portion of the code touching users may never have been seen by human eyes.

This shift presents both opportunities and challenges. On one hand, our software can become more adaptable and flexible, and allow us to tackle problems that were previously too complex or time-consuming. On the other hand, the fundamental assumptions that our software ecosystem is built upon, human review, intentional design, and careful testing, are being upended.

## Building Trust Into the System

Theater builds some guarantees into the structure of the software system itself through three key concepts:

1. **WebAssembly Components** provide sandboxing and determinism, ensuring that code operates within well-defined boundaries.
2. **Actor Model with Supervision** implements an Erlang-style supervision system, creating isolation between components and facilitating recovery from failures
3. **Complete Traceability** tracks all information that enters or leaves the WebAssembly sandbox, creating a comprehensive audit trail for debugging and verification.

## Who Is Theater For?

Theater is designed for developers and organizations who:

- Want to safely run untrusted code, including AI-generated components
- Need to build systems with high reliability and fault tolerance
- Are developing microservices that require strong isolation
- Want to learn and experiment with actor models and distributed systems

Whether you're integrating AI-generated code into your workflow, building critical systems, or simply exploring new paradigms in software architecture, Theater provides a robust framework for building more trustworthy systems.

## About This Book

This book serves as the comprehensive guide to Theater. It starts with practical getting-started guides, dives into the core concepts that underpin the system, and then explores advanced topics for those looking to push the boundaries of what's possible.

Each chapter builds upon the previous ones, but feel free to jump to the sections most relevant to your immediate needs. The examples are designed to be practical and applicable to real-world scenarios.

Let's begin exploring how Theater can transform the way we build and reason about software in the age of AI-generated code.

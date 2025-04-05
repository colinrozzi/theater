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

Theater is currently an experimental project for hobbyists, hackers, and pioneers to push the boundaries of software. It is not intended for production use at this time.

## About This Book

This book should be a friendly introduction to the Theater system. It is meant for programmers new to Theater with programming experience. For a more in-depth and precise understanding of the system and its component parts, please refer to the [reference](/theater/api/theater/index.html)


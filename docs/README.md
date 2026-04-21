# Theater Documentation

## Core Concepts

- **[Contract Enforcement](./contract-enforcement.md)** — How Theater validates type contracts between the runtime and actors, on both input and output.

## System Design

- **[Shutdown System Analysis](./shutdown-system-analysis.md)** — How actor shutdown works, including graceful vs forced shutdown paths.
- **[Shutdown Resource Ownership](./shutdown-resource-ownership.md)** — Resource lifecycle and ownership during shutdown.
- **[Handler Shutdown Analysis](./handler-shutdown-analysis.md)** — How handlers participate in shutdown.

## Overview

Theater is a WebAssembly actor runtime. Actors are WASM modules loaded via pack, with typed interfaces enforced at the boundary.

- **Actors**: WebAssembly modules with typed state and function signatures
- **Handlers**: Capabilities that actors import (message server, store, supervisor, etc.)
- **Pack**: The type system and ABI layer — encodes/decodes values, embeds type metadata
- **Event Chain**: Pack-encoded audit log of all operations
- **Contract Enforcement**: Runtime validates types before and after every WASM call

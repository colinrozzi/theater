# {{project_name}}

A Theater supervisor actor for managing child actors.

## Building

```bash
cargo component build --release
```

## Running

```bash
theater start manifest.toml
```

## Features

This supervisor actor implements:

- Child actor lifecycle management
- Error handling and restart strategies
- State tracking for supervised actors
- Supervisor hierarchy integration

## Supervision Strategy

The supervisor implements a simple restart strategy:

- Child errors trigger restart attempts
- Exit events remove children from tracking
- External stops are handled gracefully

## Usage

This actor is designed to supervise other actors in a Theater system. It can be used as a building block for more complex supervision trees.

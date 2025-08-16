# {{project_name}}

A Theater actor with message server capabilities.

## Building

```bash
cargo component build --release
```

## Running

```bash
theater start manifest.toml
```

## Features

This actor implements:

- Basic actor lifecycle
- Message server client interface
- Channel management
- State persistence

## API

The actor can handle:

- Direct messages via `handle_send`
- Request/response via `handle_request`
- Channel operations (open, close, message)

## Example Usage

```bash
# Send a message to the actor
theater message {{project_name}} "Hello, World!"

# Open a channel to the actor
theater channel open {{project_name}} --data "Channel request"
```

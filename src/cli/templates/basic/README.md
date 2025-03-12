# {{project_name}}

A basic Theater actor created from the template.

## Building

To build the actor:

```bash
cargo build --target wasm32-unknown-unknown --release
```

## Running

To run the actor with Theater:

```bash
theater start manifest.toml
```

## Features

This basic actor supports:

- Storing and retrieving state
- Handling simple messages
- Incrementing a counter
- Storing text messages

## API

You can interact with this actor using the following commands:

- `count` - Get the current count
- `messages` - Get all stored messages
- `increment` - Increment the counter
- Any other text - Store as a message

## Example

```bash
# Send a request to get the current count
theater message {{project_name}} count
```

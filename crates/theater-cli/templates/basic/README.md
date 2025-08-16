# {{project_name}}

A basic Theater actor created from the template.

## Building

To build the actor:

```bash
cargo component build --release
```

## Running

To run the actor with Theater:

```bash
theater start manifest.toml
```

## Features

This basic actor supports:

- State management with serialization
- Initialization logging
- Runtime integration

## Development

The actor implements the `theater:simple/actor` interface and can be extended with additional capabilities.

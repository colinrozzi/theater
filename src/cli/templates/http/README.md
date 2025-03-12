# {{project_name}}

An HTTP server Theater actor created from the template.

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

This HTTP actor provides:

- RESTful API endpoints
- WebSocket support
- Middleware for authentication
- State management
- Message handling

## API Endpoints

The actor exposes the following HTTP endpoints:

- `GET /api/count` - Get the current count
- `POST /api/count` - Increment the count
- `GET /api/messages` - Get all stored messages
- `POST /api/messages` - Add a new message

## WebSocket

The actor also supports WebSocket connections at `/ws`. The WebSocket interface:

- Echoes back text messages
- Returns the current count
- Stores new messages in the actor state

## Authentication

API endpoints under `/api/*` are protected with a simple API key middleware.
Include the header `X-API-Key: theater-demo-key` in your requests.

## Example Usage

```bash
# Get the current count
curl -H "X-API-Key: theater-demo-key" http://localhost:8080/api/count

# Add a new message
curl -X POST -H "Content-Type: application/json" \
     -H "X-API-Key: theater-demo-key" \
     -d '{"message":"Hello, Theater!"}' \
     http://localhost:8080/api/messages
```

For WebSocket testing, you can use a tool like websocat:

```bash
websocat ws://localhost:8080/ws
```

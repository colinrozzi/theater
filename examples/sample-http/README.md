# Sample HTTP Server with Theater

This is a sample WebAssembly actor that demonstrates the HTTP Framework in Theater.

## Features

- **HTTP API**: Simple REST API with GET and POST endpoints
- **Middleware**: Authentication middleware using API keys
- **WebSocket**: Real-time communication with message echo
- **State Management**: Persistent state between requests

## Building

```bash
# Build the WebAssembly module
cargo build --target wasm32-unknown-unknown --release
```

## Running

```bash
# Navigate to the theater directory
cd ../theater

# Run the actor with the manifest
cargo run -- --manifest ../actors/sample-http/manifest.toml
```

## API Endpoints

### GET /api/count
Returns the current count value.

**Example response:**
```json
{
  "count": 5
}
```

### POST /api/count
Increments the count value and returns the new count.

**Example response:**
```json
{
  "count": 6
}
```

### GET /api/messages
Returns all messages received via POST or WebSocket.

**Example response:**
```json
{
  "messages": [
    "Hello, world!",
    "This is a test message"
  ]
}
```

### POST /api/messages
Adds a new message to the collection.

**Example request:**
```json
{
  "message": "Hello, Theater!"
}
```

**Example response:**
```json
{
  "success": true,
  "message": "Message added successfully"
}
```

## Authentication

All API endpoints require authentication using the `X-API-Key` header:

```
X-API-Key: theater-demo-key
```

## WebSocket

Connect to the WebSocket endpoint at `/ws`.

- Send a text message to echo it back
- The server will also send the current count with each message
- All messages sent via WebSocket are stored in the state

# API Documentation

## HTTP API

### POST /
Process a message and update system state.

#### Request
- Method: `POST`
- Content-Type: `application/json`
- Body:
```json
{
    "data": <JSON Value>
}
```

#### Response
- Status: 200 OK
- Content-Type: `application/json`
- Body:
```json
{
    "hash": "<state_hash>",
    "state": <JSON Value>
}
```

#### Error Responses
- Status: 400 Bad Request
  - Invalid message format
  - Contract violation
  - State verification failure
- Status: 500 Internal Server Error
  - Runtime errors
  - WASM execution errors

### GET /chain
Retrieve the complete hash chain.

#### Request
- Method: `GET`
- No request body required

#### Response
- Status: 200 OK
- Content-Type: `application/json`
- Body:
```json
{
    "head": "current-head-hash",
    "entries": [
        ["hash1", {
            "parent": "hash2",
            "data": <JSON Value>
        }],
        ["hash2", {
            "parent": "hash3",
            "data": <JSON Value>
        }],
        ["hash3", {
            "parent": null,
            "data": <JSON Value>
        }]
    ]
}
```

Notes:
- Entries are ordered from newest to oldest
- The genesis block has `null` as its parent
- Each entry is a tuple of [hash, entry_data]

## Actor Interface

### Component Interface
Required implementation for WASM components.

#### `init() -> Value`
Initialize component state.
- Returns: Initial state as JSON value

#### `handle(msg: Value, state: Value) -> Value`
Process a message and update state.
- Parameters:
  - msg: Message data as JSON
  - state: Current state as JSON
- Returns: New state as JSON value

#### `message_contract(msg: Value, state: Value) -> bool`
Verify message validity.
- Parameters:
  - msg: Message to verify
  - state: Current state
- Returns: true if valid, false if invalid

#### `state_contract(state: Value) -> bool`
Verify state validity.
- Parameters:
  - state: State to verify
- Returns: true if valid, false if invalid

### Host Functions
Functions provided by the runtime to components.

#### `log(msg: &str)`
Log a message from the component.
- Parameters:
  - msg: Message to log

#### `send(actor_id: &str, msg: &Value)`
Send a message to another actor.
- Parameters:
  - actor_id: Target actor identifier
  - msg: Message to send

## Usage Examples

### Send Message
```bash
curl -X POST http://localhost:8080/ \
     -H "Content-Type: application/json" \
     -d '{
           "data": {
             "action": "update",
             "value": 42
           }
         }'
```

### Get Chain
```bash
curl http://localhost:8080/chain
```

### Response Examples

#### POST / Response
```json
{
    "hash": "7f83b1657ff1fc53b92dc18148a1d65dfc2d4b1fa3d677284addd200126d9069",
    "state": {
        "counter": 42,
        "last_update": "2024-12-04T10:00:00Z"
    }
}
```

#### GET /chain Response
```json
{
    "head": "7f83b1657ff1fc53b92dc18148a1d65dfc2d4b1fa3d677284addd200126d9069",
    "entries": [
        ["7f83b...9069", {
            "parent": "6d23c...8901",
            "data": {
                "counter": 42,
                "last_update": "2024-12-04T10:00:00Z"
            }
        }],
        ["6d23c...8901", {
            "parent": null,
            "data": {
                "component_hash": "####"
            }
        }]
    ]
}
```

## Best Practices

### Chain Management
1. Regularly verify chain integrity
2. Monitor chain growth
3. Consider data storage implications
4. Cache frequently accessed states

### Message Design
1. Use clear action identifiers
2. Include necessary data only
3. Validate before sending
4. Handle errors appropriately

### State Management
1. Keep states minimal
2. Validate all transitions
3. Maintain data integrity
4. Consider performance impact

### Error Handling
1. Check response status
2. Parse error messages
3. Implement retries when appropriate
4. Log failures for debugging

## Rate Limiting
- No explicit rate limiting currently implemented
- Consider application-level throttling
- Monitor system resources

## Security Notes
1. Local-only HTTP server
2. No authentication currently
3. Input validation required
4. Contract enforcement critical
5. Chain integrity verification important
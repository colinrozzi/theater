# Improve Actor Communication Module Naming

## Current State
Currently, we have two HTTP-related modules with potentially confusing names:
- `http.rs`: Handles actor-to-actor communication
- `http_server.rs`: Handles external HTTP requests

This naming scheme is unclear and could lead to confusion about the purpose of each module.

## Proposed Changes

### 1. Rename Modules
```
http.rs -> actor_messenger.rs
http_server.rs -> http_handler.rs
```

### 2. Update Type Names
- `HttpHost` -> `ActorMessenger`
- `HttpHandler` -> `MessengerHandler`
- `HttpServerHost` -> `HttpHost`
- `HttpServerHandler` -> `HttpHandler`

### 3. Update Configuration Keys
In actor manifests:
```toml
# Before
[[handlers]]
type = "Http"
config = { port = 8080 }

# After
[[handlers]]
type = "Messenger"
config = { port = 8080 }
```

## Benefits
1. Clear separation of concerns in naming
2. More accurate representation of module purposes
3. Reduced confusion for new developers
4. Better alignment with actual functionality

## Implementation Notes
1. Create new files with updated names
2. Migrate code with new type names
3. Update all references in other modules
4. Update documentation
5. Maintain backward compatibility during transition
6. Add deprecation notices for old names

## Migration Strategy
1. Add new modules alongside existing ones
2. Support both old and new names temporarily
3. Mark old names as deprecated
4. Remove old modules in next major version
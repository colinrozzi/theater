# Configuration Reference

Theater uses TOML for configuration, with support for both actor manifests and system configuration.

## Actor Manifest

The basic structure of an actor manifest:

```toml
# Basic actor information
name = "my-actor"
component_path = "path/to/actor.wasm"

# Interface definitions
[interface]
implements = [
    "ntwk:simple-actor/actor",
    "ntwk:simple-actor/http-server"
]
requires = ["ntwk:simple-actor/http-client"]

# Handler configurations
[[handlers]]
type = "Http-server"
config = { port = 8080 }

[[handlers]]
type = "Metrics"
config = { path = "/metrics" }
```

### Core Fields

- `name` (required): Unique identifier for the actor
- `component_path` (required): Path to the WebAssembly component
- `description` (optional): Human-readable description
- `version` (optional): Semantic version of the actor

### Interface Configuration

```toml
[interface]
# Interfaces this actor implements
implements = [
    "ntwk:simple-actor/actor",     # Core actor interface
    "ntwk:simple-actor/metrics",   # Metrics exposure
    "my-org:custom/interface"      # Custom interfaces
]

# Interfaces this actor requires
requires = [
    "ntwk:simple-actor/http-client"
]

# Optional interface configuration
[interface.config]
timeout_ms = 5000
retry_count = 3
```

### Handler Types

#### HTTP Server
```toml
[[handlers]]
type = "Http-server"
config = {
    port = 8080,
    host = "127.0.0.1",  # Optional, defaults to 0.0.0.0
    path_prefix = "/api", # Optional base path
    
    # Optional TLS configuration
    tls = {
        cert_path = "/path/to/cert.pem",
        key_path = "/path/to/key.pem"
    }
}
```

#### HTTP Client
```toml
[[handlers]]
type = "Http-client"
config = {
    base_url = "https://api.example.com",
    timeout_ms = 5000,
    
    # Optional default headers
    headers = {
        "User-Agent" = "Theater/1.0",
        "Authorization" = "Bearer ${ENV_TOKEN}"
    }
}
```

#### Metrics Handler
```toml
[[handlers]]
type = "Metrics"
config = {
    path = "/metrics",
    port = 9090,        # Optional, uses main HTTP port if not specified
    format = "prometheus"
}
```

#### Custom Handler
```toml
[[handlers]]
type = "Custom"
name = "my-handler"
config = {
    # Handler-specific configuration
    setting1 = "value1",
    setting2 = 42
}
```

### State Configuration

```toml
[state]
# Initial state as JSON
initial = """
{
    "count": 0,
    "created_at": "${NOW}"
}
"""

# Optional state validation
[state.validation]
schema = "path/to/schema.json"
```

### Environment Variables

Configuration values can reference environment variables:
- `${ENV_NAME}`: Required environment variable
- `${ENV_NAME:-default}`: Environment variable with default
- `${NOW}`: Current timestamp
- `${UUID}`: Generate unique ID

Example:
```toml
name = "actor-${ENV_INSTANCE_ID:-001}"
component_path = "${COMPONENT_DIR}/actor.wasm"

[interface.config]
api_key = "${API_KEY}"
```

## System Configuration

Theater system-wide configuration:

```toml
# System configuration file: theater.toml

[system]
# Directory for WebAssembly components
component_dir = "/opt/theater/components"

# Logging configuration
[system.logging]
level = "info"
format = "json"
output = "stdout"

# Hash chain storage
[system.storage]
type = "filesystem"
path = "/var/lib/theater/chains"

# Optional distributed configuration
[system.distributed]
discovery = "consul"
consul_url = "http://localhost:8500"
```

### Actor Loading

```toml
[system.loading]
# Allow actors to be loaded from these directories
allowed_paths = [
    "/opt/theater/components",
    "${HOME}/.theater/components"
]

# Component validation
verify_signatures = true
signature_keys = ["path/to/public.key"]
```

### Resource Limits

```toml
[system.limits]
# Memory limits
max_memory_mb = 512
max_state_size_mb = 10

# Execution limits
max_execution_time_ms = 1000
max_message_size_kb = 64

# Handler limits
max_http_connections = 1000
max_handlers_per_actor = 5
```

### Security Settings

```toml
[system.security]
# WASM execution
enable_bulk_memory = true
enable_threads = false
enable_simd = false

# Network access
allow_outbound = true
allowed_hosts = [
    "api.example.com",
    "*.internal.org"
]

# File system access
allow_fs_read = true
allow_fs_write = false
allowed_paths = ["/var/lib/theater"]
```

## Development Configuration

For development environments:

```toml
[dev]
# Hot reload configuration
watch_paths = ["src", "components"]
reload_on_change = true

# Development-specific handlers
[[dev.handlers]]
type = "Http-server"
config = { port = 3000 }

# Mock external services
[[dev.mocks]]
name = "external-api"
port = 8081
responses = "path/to/mock/responses.json"
```

## Best Practices

1. **Configuration Organization**
   - Keep configurations in dedicated directory
   - Use environment variables for secrets
   - Version control templates, not actual configs
   - Document all custom values

2. **Security**
   - Never commit sensitive values
   - Use environment variables for credentials
   - Restrict file system access
   - Limit network access

3. **Development**
   - Use separate dev configurations
   - Enable detailed logging
   - Configure mock services
   - Set reasonable resource limits

4. **Deployment**
   - Use environment-specific configs
   - Validate all configurations
   - Monitor resource limits
   - Document all settings

5. **Maintenance**
   - Regular config reviews
   - Update security settings
   - Clean up unused configs
   - Track config changes
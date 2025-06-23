# theater-server

HTTP server component for the Theater WebAssembly actor system.

## Overview

`theater-server` provides the HTTP API server for managing Theater actors. It handles:

- Actor lifecycle management via REST API
- WebSocket connections for real-time event streaming
- Server administration and monitoring

## Usage

```rust
use theater_server::{ServerConfig, start_server};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ServerConfig::default();
    start_server(config).await?;
    Ok(())
}
```

## API Endpoints

The server provides REST endpoints for:
- `/actors` - Actor management
- `/events` - Event streaming
- `/health` - Server health checks

For complete API documentation, see the [Theater Guide](https://colinrozzi.github.io/theater/guide).

## License

This project is licensed under the Apache License 2.0 - see the [LICENSE](../../LICENSE) file for details.

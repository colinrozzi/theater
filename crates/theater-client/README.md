# theater-client

Client library for the Theater WebAssembly actor system.

## Overview

`theater-client` provides a Rust client library for interacting with Theater servers. It offers both synchronous and asynchronous APIs for managing actors and streaming events.

## Usage

```rust
use theater_client::{TheaterClient, ClientConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = TheaterClient::new("http://localhost:8080")?;
    
    // List running actors
    let actors = client.list_actors().await?;
    println!("Running actors: {:#?}", actors);
    
    // Start an actor
    let actor_id = client.start_actor("manifest.toml").await?;
    println!("Started actor: {}", actor_id);
    
    Ok(())
}
```

## Features

- Async/await support with tokio
- Type-safe API responses
- Event streaming capabilities
- Connection management and retries

For complete documentation, see the [Theater Guide](https://colinrozzi.github.io/theater/guide).

## License

This project is licensed under the Apache License 2.0 - see the [LICENSE](../../LICENSE) file for details.

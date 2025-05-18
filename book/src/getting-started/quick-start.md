# Quick Start Guide

This guide will help you get up and running with Theater as quickly as possible. We'll create a simple actor, build it, and run it using the Theater CLI.

## Prerequisites

Before you begin, make sure you have:

- Rust 1.81.0 or newer installed
- Cargo installed and working
- Git installed

## Installation

First, let's install the Theater CLI. The simplest way is to build it from source:

```bash
# Clone the repository
git clone https://github.com/colinrozzi/theater.git
cd theater

# Build and install the CLI
cargo install --path .
```

Verify the installation by running:

```bash
theater --version
```

You should see the version of Theater printed to the console.

## Creating Your First Actor

Now, let's create a simple "hello world" actor:

```bash
# Create a new actor project
theater create hello-actor
cd hello-actor
```

This will generate a new Rust project with the necessary structure for a Theater actor.

## Exploring the Project Structure

The generated project has the following structure:

```
hello-actor/
├── Cargo.toml           # Rust package definition
├── manifest.toml        # Actor manifest file
├── wit/                 # WebAssembly interface definitions
│   └── deps/            # Interface dependencies
└── src/
    └── lib.rs           # Actor implementation
```

Let's examine the `src/lib.rs` file:

```rust
use theater_bindgen::prelude::*;

pub struct Component;

impl Component {
    pub fn new() -> Self {
        Self
    }
}

impl message_server::MessageServer for Component {
    fn handle_request(&mut self, message: String) -> Result<String, String> {
        let response = format!("Hello! You sent: {}", message);
        Ok(response)
    }
}

theater_bindgen::export!(Component);
```

This simple actor implements the `message-server` interface, which allows it to receive and respond to messages.

## Building the Actor

To compile your actor to WebAssembly, run:

```bash
theater build
```

This will create a WebAssembly component file in the `target/wasm32-wasi/release` directory.

## Starting a Theater Server

Before we can run our actor, we need to start a Theater server:

```bash
# In a separate terminal window
theater server
```

You should see output indicating that the server has started and is listening for connections.

## Running Your Actor

Now, let's start our actor:

```bash
theater start manifest.toml
```

If successful, you'll see output showing that the actor has been started, along with its unique identifier.

## Interacting with Your Actor

Let's send a message to our actor and see its response:

```bash
# Get the actor ID (replace with your actual actor ID)
ACTOR_ID=$(theater list | grep hello-actor | awk '{print $1}')

# Send a message to the actor
theater send $ACTOR_ID "Hello, Theater!"
```

You should receive a response like:

```
Hello! You sent: Hello, Theater!
```

## Monitoring Your Actor

Theater provides several ways to monitor your actors:

```bash
# List all running actors
theater list

# View logs for your actor
theater logs $ACTOR_ID

# Subscribe to events from your actor
theater subscribe $ACTOR_ID
```

The `subscribe` command is particularly useful as it shows you all events in real-time.

## Modifying Your Actor

Now, let's make a simple change to our actor. Open `src/lib.rs` in your favorite editor and modify the `handle_message` function:

```rust
fn handle_message(&mut self, message: String) -> Result<String, String> {
    let response = format!("Greetings from Theater! You sent: {}", message);
    Ok(response)
}
```

Rebuild and restart your actor:

```bash
theater build
theater stop $ACTOR_ID  # Stop the previous instance
theater start manifest.toml
```

Now send another message and observe the different response:

```bash
ACTOR_ID=$(theater list | grep hello-actor | awk '{print $1}')
theater send $ACTOR_ID "Hello again!"
```

## Creating a More Interesting Actor

Let's build a slightly more complex actor that maintains state. Replace the contents of `src/lib.rs` with:

```rust
use theater_bindgen::prelude::*;
use serde::{Serialize, Deserialize};

#[derive(Default, Serialize, Deserialize)]
pub struct Counter {
    count: i32,
}

impl Counter {
    pub fn new() -> Self {
        Self::default()
    }
}

impl message_server::MessageServer for Counter {
    fn handle_message(&mut self, message: String) -> Result<String, String> {
        match message.as_str() {
            "increment" => {
                self.count += 1;
                Ok(format!("Counter incremented to {}", self.count))
            }
            "decrement" => {
                self.count -= 1;
                Ok(format!("Counter decremented to {}", self.count))
            }
            "get" => {
                Ok(format!("Current count is {}", self.count))
            }
            _ => {
                Err("Unknown command. Try 'increment', 'decrement', or 'get'.".to_string())
            }
        }
    }
}

theater_bindgen::export!(Counter);
```

Update your `Cargo.toml` to include the serde dependency:

```toml
[dependencies]
theater-bindgen = { path = "../theater/crates/theater-bindgen" }
serde = { version = "1.0", features = ["derive"] }
```

Then rebuild and restart your actor:

```bash
theater build
theater stop $ACTOR_ID  # Stop the previous instance
theater start manifest.toml
```

Now you can interact with your counter:

```bash
ACTOR_ID=$(theater list | grep hello-actor | awk '{print $1}')
theater send $ACTOR_ID "increment"
theater send $ACTOR_ID "increment"
theater send $ACTOR_ID "get"
theater send $ACTOR_ID "decrement"
theater send $ACTOR_ID "get"
```

## Exploring State History

One of Theater's powerful features is the ability to track state history. Let's see it in action:

```bash
# View the state history of your actor
theater state-history $ACTOR_ID
```

You should see a complete history of all state changes, showing how the counter value changed over time.

## Next Steps

Now that you've created your first actor and seen some of Theater's basic features, you might want to:

1. Learn about [creating more complex actors](first-actor.md)
2. Understand the [supervision system](../concepts/supervision.md)
3. Explore how to [create actor hierarchies](../development/interfaces.md)
4. Check out the [CLI reference](../cli/overview.md) for more commands

Theater provides a powerful platform for building reliable, traceable systems. As you grow more comfortable with its core concepts, you'll be able to leverage its full potential for your applications.

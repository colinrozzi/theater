use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;
use tracing::{debug, info};

/// Template metadata
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Template {
    pub name: String,
    pub description: String,
    pub files: HashMap<&'static str, &'static str>,
}

/// Available templates for creating new actors
pub fn available_templates() -> HashMap<String, Template> {
    let mut templates = HashMap::new();

    // Basic actor template
    templates.insert(
        "basic".to_string(),
        Template {
            name: "basic".to_string(),
            description: "A simple Theater actor with basic functionality".to_string(),
            files: basic_template_files(),
        },
    );

    // Message server actor template
    templates.insert(
        "message-server".to_string(),
        Template {
            name: "message-server".to_string(),
            description: "A Theater actor with message server capabilities".to_string(),
            files: message_server_template_files(),
        },
    );

    // Supervisor actor template
    templates.insert(
        "supervisor".to_string(),
        Template {
            name: "supervisor".to_string(),
            description: "A Theater actor with supervisor capabilities for managing child actors".to_string(),
            files: supervisor_template_files(),
        },
    );

    templates
}

/// Basic actor template files (like hello-world)
fn basic_template_files() -> HashMap<&'static str, &'static str> {
    let mut files = HashMap::new();

    // Cargo.toml
    files.insert("Cargo.toml", r#"[package]
name = "{{project_name}}"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
wit-bindgen-rt = { version = "0.43.0", features = ["bitflags"] }

[package.metadata.component]
package = "component:{{project_name}}"

[package.metadata.component.target.dependencies]
"theater:simple" = { path = "./wit/deps/theater-simple" }

[package.metadata.component.bindings]
derives = ["serde::Serialize", "serde::Deserialize", "PartialEq"]
generate_unused_types = true
"#);

    // manifest.toml
    files.insert("manifest.toml", r#"name = "{{project_name}}"
version = "0.1.0"
component = "./target/wasm32-unknown-unknown/release/{{project_name_snake}}.wasm"
description = "A basic Theater actor"
save_chain = true

[[handlers]]
type = "runtime"

[handlers.config]
"#);

    // wit/world.wit
    files.insert("wit/world.wit", r#"package component:{{project_name}};

world default {
    import theater:simple/runtime;
    export theater:simple/actor;
}
"#);

    // src/lib.rs
    files.insert("src/lib.rs", r#"#[allow(warnings)]
mod bindings;

use bindings::exports::theater::simple::actor::Guest;
use bindings::theater::simple::runtime::{log, shutdown};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Default)]
struct ActorState {
    counter: u32,
    messages: Vec<String>,
}

struct Component;

impl Guest for Component {
    fn init(
        state: Option<Vec<u8>>,
        params: (String,),
    ) -> Result<(Option<Vec<u8>>,), String> {
        log("Initializing {{project_name}} actor");
        let (self_id,) = params;
        log(&format!("Actor ID: {}", &self_id));
        log("Hello from {{project_name}} actor!");

        // Parse existing state or create new
        let actor_state = match state {
            Some(bytes) => {
                serde_json::from_slice::<ActorState>(&bytes)
                    .unwrap_or_else(|_| ActorState::default())
            }
            None => ActorState::default(),
        };

        // Serialize state back
        let new_state = serde_json::to_vec(&actor_state)
            .map_err(|e| format!("Failed to serialize state: {}", e))?;

        // For demo, we'll shutdown after init - remove this for persistent actors
        shutdown(None);

        Ok((Some(new_state),))
    }
}

bindings::export!(Component with_types_in bindings);
"#);

    // README.md
    files.insert("README.md", r#"# {{project_name}}

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
"#);

    // wkg.toml for dependency management
    files.insert("wkg.toml", r#"[metadata]
name = "{{project_name}}"
version = "0.1.0"

[dependencies]
"theater:simple" = "*"
"#);

    files
}

/// Message server actor template files
fn message_server_template_files() -> HashMap<&'static str, &'static str> {
    let mut files = HashMap::new();

    // Cargo.toml
    files.insert("Cargo.toml", r#"[package]
name = "{{project_name}}"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
wit-bindgen-rt = { version = "0.43.0", features = ["bitflags"] }

[package.metadata.component]
package = "component:{{project_name}}"

[package.metadata.component.target.dependencies]
"theater:simple" = { path = "./wit/deps/theater-simple" }

[package.metadata.component.bindings]
derives = ["serde::Serialize", "serde::Deserialize", "PartialEq"]
generate_unused_types = true
"#);

    // manifest.toml
    files.insert("manifest.toml", r#"name = "{{project_name}}"
version = "0.1.0"
component = "./target/wasm32-unknown-unknown/release/{{project_name_snake}}.wasm"
description = "A Theater actor with message server capabilities"
save_chain = true

[[handlers]]
type = "runtime"

[handlers.config]
"#);

    // wit/world.wit
    files.insert("wit/world.wit", r#"package component:{{project_name}};

world default {
    import theater:simple/runtime;
    export theater:simple/actor;
    export theater:simple/message-server-client;
}
"#);

    // src/lib.rs
    files.insert("src/lib.rs", r#"#[allow(warnings)]
mod bindings;

use bindings::exports::theater::simple::actor::Guest;
use bindings::exports::theater::simple::message_server_client::Guest as MessageServerClient;
use bindings::theater::simple::runtime::log;
use bindings::theater::simple::types::{ChannelAccept, ChannelId};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Default)]
struct ActorState {
    messages: Vec<String>,
    channels: Vec<String>,
}

struct Component;

impl Guest for Component {
    fn init(
        state: Option<Vec<u8>>,
        params: (String,),
    ) -> Result<(Option<Vec<u8>>,), String> {
        log("Initializing {{project_name}} message server actor");
        let (self_id,) = params;
        log(&format!("Actor ID: {}", &self_id));

        // Parse existing state or create new
        let actor_state = match state {
            Some(bytes) => {
                serde_json::from_slice::<ActorState>(&bytes)
                    .unwrap_or_else(|_| ActorState::default())
            }
            None => ActorState::default(),
        };

        // Serialize state back
        let new_state = serde_json::to_vec(&actor_state)
            .map_err(|e| format!("Failed to serialize state: {}", e))?;

        Ok((Some(new_state),))
    }
}

impl MessageServerClient for Component {
    fn handle_send(
        state: Option<Vec<u8>>,
        params: (Vec<u8>,),
    ) -> Result<(Option<Vec<u8>>,), String> {
        let (data,) = params;
        log(&format!("Received message: {} bytes", data.len()));
        
        // Parse and update state
        let mut actor_state: ActorState = match state {
            Some(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            None => ActorState::default(),
        };
        
        // Store message (as string if possible)
        if let Ok(msg) = String::from_utf8(data) {
            actor_state.messages.push(msg);
        }
        
        let new_state = serde_json::to_vec(&actor_state)
            .map_err(|e| format!("Failed to serialize state: {}", e))?;

        Ok((Some(new_state),))
    }

    fn handle_request(
        state: Option<Vec<u8>>,
        params: (String, Vec<u8>),
    ) -> Result<(Option<Vec<u8>>, (Option<Vec<u8>>,)), String> {
        let (request_id, data) = params;
        log(&format!("Received request {}: {} bytes", request_id, data.len()));
        
        // Echo the data back as response
        Ok((state, (Some(data),)))
    }

    fn handle_channel_open(
        state: Option<Vec<u8>>,
        params: (String, Vec<u8>),
    ) -> Result<(Option<Vec<u8>>, (ChannelAccept,)), String> {
        let (channel_id, _data) = params;
        log(&format!("Channel open request: {}", channel_id));
        
        // Accept all channel requests
        Ok((
            state,
            (ChannelAccept {
                accepted: true,
                message: Some(b"Welcome to the channel!".to_vec()),
            },),
        ))
    }

    fn handle_channel_close(
        state: Option<Vec<u8>>,
        params: (ChannelId,),
    ) -> Result<(Option<Vec<u8>>,), String> {
        let (channel_id,) = params;
        log(&format!("Channel closed: {}", channel_id));
        Ok((state,))
    }

    fn handle_channel_message(
        state: Option<Vec<u8>>,
        params: (ChannelId, Vec<u8>),
    ) -> Result<(Option<Vec<u8>>,), String> {
        let (channel_id, data) = params;
        log(&format!("Channel {} message: {} bytes", channel_id, data.len()));
        Ok((state,))
    }
}

bindings::export!(Component with_types_in bindings);
"#);

    // README.md
    files.insert("README.md", r#"# {{project_name}}

A Theater actor with message server capabilities.

## Building

```bash
cargo component build --release
```

## Running

```bash
theater start manifest.toml
```

## Features

This actor implements:

- Basic actor lifecycle
- Message server client interface
- Channel management
- State persistence

## API

The actor can handle:

- Direct messages via `handle_send`
- Request/response via `handle_request`
- Channel operations (open, close, message)

## Example Usage

```bash
# Send a message to the actor
theater message {{project_name}} "Hello, World!"

# Open a channel to the actor
theater channel open {{project_name}} --data "Channel request"
```
"#);

    // wkg.toml for dependency management
    files.insert("wkg.toml", r#"[metadata]
name = "{{project_name}}"
version = "0.1.0"

[dependencies]
"theater:simple" = "*"
"#);

    files
}

/// Supervisor actor template files
fn supervisor_template_files() -> HashMap<&'static str, &'static str> {
    let mut files = HashMap::new();

    // Cargo.toml
    files.insert("Cargo.toml", r#"[package]
name = "{{project_name}}"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
wit-bindgen-rt = { version = "0.43.0", features = ["bitflags"] }

[package.metadata.component]
package = "component:{{project_name}}"

[package.metadata.component.target.dependencies]
"theater:simple" = { path = "./wit/deps/theater-simple" }

[package.metadata.component.bindings]
derives = ["serde::Serialize", "serde::Deserialize", "PartialEq"]
generate_unused_types = true
"#);

    // manifest.toml
    files.insert("manifest.toml", r#"name = "{{project_name}}"
version = "0.1.0"
component = "./target/wasm32-unknown-unknown/release/{{project_name_snake}}.wasm"
description = "A Theater supervisor actor"
save_chain = true

[[handlers]]
type = "runtime"

[handlers.config]
"#);

    // wit/world.wit
    files.insert("wit/world.wit", r#"package component:{{project_name}};

world default {
    import theater:simple/runtime;
    import theater:simple/supervisor;
    export theater:simple/actor;
    export theater:simple/supervisor-handlers;
}
"#);

    // src/lib.rs
    files.insert("src/lib.rs", r#"#[allow(warnings)]
mod bindings;

use bindings::exports::theater::simple::actor::Guest;
use bindings::exports::theater::simple::supervisor_handlers::Guest as SupervisorHandlers;
use bindings::theater::simple::runtime::log;
use bindings::theater::simple::supervisor;
use bindings::theater::simple::types::WitActorError;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Default)]
struct SupervisorState {
    children: Vec<String>,
    restart_count: u32,
}

struct Component;

impl Guest for Component {
    fn init(
        state: Option<Vec<u8>>,
        params: (String,),
    ) -> Result<(Option<Vec<u8>>,), String> {
        log("Initializing {{project_name}} supervisor actor");
        let (self_id,) = params;
        log(&format!("Supervisor ID: {}", &self_id));

        // Parse existing state or create new
        let supervisor_state = match state {
            Some(bytes) => {
                serde_json::from_slice::<SupervisorState>(&bytes)
                    .unwrap_or_else(|_| SupervisorState::default())
            }
            None => SupervisorState::default(),
        };

        // Serialize state back
        let new_state = serde_json::to_vec(&supervisor_state)
            .map_err(|e| format!("Failed to serialize state: {}", e))?;

        Ok((Some(new_state),))
    }
}

impl SupervisorHandlers for Component {
    fn handle_child_error(
        state: Option<Vec<u8>>,
        params: (String, WitActorError),
    ) -> Result<(Option<Vec<u8>>,), String> {
        let (child_id, error) = params;
        log(&format!("Child actor {} error: {:?}", child_id, error));
        
        // Parse state
        let mut supervisor_state: SupervisorState = match state {
            Some(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            None => SupervisorState::default(),
        };
        
        // Implement restart strategy
        supervisor_state.restart_count += 1;
        log(&format!("Restarting child {} (attempt {})", child_id, supervisor_state.restart_count));
        
        // TODO: Implement actual restart logic using supervisor interface
        
        let new_state = serde_json::to_vec(&supervisor_state)
            .map_err(|e| format!("Failed to serialize state: {}", e))?;
        
        Ok((Some(new_state),))
    }

    fn handle_child_exit(
        state: Option<Vec<u8>>,
        params: (String, Option<Vec<u8>>),
    ) -> Result<(Option<Vec<u8>>,), String> {
        let (child_id, _exit_data) = params;
        log(&format!("Child actor exited: {}", child_id));
        
        // Parse state
        let mut supervisor_state: SupervisorState = match state {
            Some(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            None => SupervisorState::default(),
        };
        
        // Remove child from tracking
        supervisor_state.children.retain(|id| id != &child_id);
        
        let new_state = serde_json::to_vec(&supervisor_state)
            .map_err(|e| format!("Failed to serialize state: {}", e))?;
        
        Ok((Some(new_state),))
    }

    fn handle_child_external_stop(
        state: Option<Vec<u8>>,
        params: (String,),
    ) -> Result<(Option<Vec<u8>>,), String> {
        let (child_id,) = params;
        log(&format!("Child actor externally stopped: {}", child_id));
        
        // Parse state
        let mut supervisor_state: SupervisorState = match state {
            Some(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            None => SupervisorState::default(),
        };
        
        // Remove child from tracking
        supervisor_state.children.retain(|id| id != &child_id);
        
        let new_state = serde_json::to_vec(&supervisor_state)
            .map_err(|e| format!("Failed to serialize state: {}", e))?;
        
        Ok((Some(new_state),))
    }
}

bindings::export!(Component with_types_in bindings);
"#);

    // README.md
    files.insert("README.md", r#"# {{project_name}}

A Theater supervisor actor for managing child actors.

## Building

```bash
cargo component build --release
```

## Running

```bash
theater start manifest.toml
```

## Features

This supervisor actor implements:

- Child actor lifecycle management
- Error handling and restart strategies
- State tracking for supervised actors
- Supervisor hierarchy integration

## Supervision Strategy

The supervisor implements a simple restart strategy:

- Child errors trigger restart attempts
- Exit events remove children from tracking
- External stops are handled gracefully

## Usage

This actor is designed to supervise other actors in a Theater system. It can be used as a building block for more complex supervision trees.
"#);

    // wkg.toml for dependency management
    files.insert("wkg.toml", r#"[metadata]
name = "{{project_name}}"
version = "0.1.0"

[dependencies]
"theater:simple" = "*"
"#);

    files
}

/// Create a new actor project from a template
pub fn create_project(
    template_name: &str,
    project_name: &str,
    target_dir: &Path,
) -> Result<(), io::Error> {
    let templates = available_templates();
    let template = templates
        .get(template_name)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Template not found"))?;

    info!(
        "Creating new {} project '{}' in {}",
        template_name,
        project_name,
        target_dir.display()
    );

    // Create the target directory
    fs::create_dir_all(target_dir)?;

    // Create all template files
    for (relative_path, content) in &template.files {
        let file_path = target_dir.join(relative_path);

        // Create parent directories if they don't exist
        if let Some(parent) = file_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        // Replace template variables
        let processed_content = content
            .replace("{{project_name}}", project_name)
            .replace("{{project_name_snake}}", &project_name.replace('-', "_"));

        debug!(
            "Creating file: {} ({} bytes)",
            file_path.display(),
            processed_content.len()
        );

        // Write the file
        fs::write(&file_path, processed_content)?;
    }

    // Note: We don't automatically create the wit/deps/theater-simple directory
    // as this should be managed by wkg or similar package manager
    info!("Project '{}' created successfully!", project_name);
    info!("Note: You may need to run 'wkg deps' or similar to fetch WIT dependencies");
    
    Ok(())
}

/// List all available templates
pub fn list_templates() {
    let templates = available_templates();
    
    println!("Available templates:");
    for (name, template) in templates {
        println!("  {}: {}", name, template.description);
    }
}

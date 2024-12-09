use anyhow::Result;
use serde_json::Value;

mod chain;
mod wasm;

pub use wasm::{WasmActor, WasmError};

// Core types that represent different kinds of actor interactions
#[derive(Debug, Clone)]
pub enum ActorInput {
    /// Regular actor-to-actor messages
    Message(Value),

    /// HTTP requests
    HttpRequest {
        method: String,
        uri: String,
        headers: Vec<(String, String)>,
        body: Option<Vec<u8>>,
    },
    // Future input types go here
}

#[derive(Debug, Clone)]
pub enum ActorOutput {
    /// Regular actor-to-actor messages
    Message(Value),

    /// HTTP responses
    HttpResponse {
        status: u16,
        headers: Vec<(String, String)>,
        body: Option<Vec<u8>>,
    },
    // Future output types go here
}

/// Core trait that all actors must implement
pub trait Actor {
    /// Initialize the actor and return its initial state
    fn init(&self) -> Result<Value>;

    /// Handle an input and return the output along with the new state
    fn handle_input(&self, input: ActorInput, state: &Value) -> Result<(ActorOutput, Value)>;

    /// Verify that a given state is valid for this actor
    fn verify_state(&self, state: &Value) -> bool;
}

/// The core runtime that manages state and the chain
pub struct ActorRuntime<A: Actor> {
    actor: A,
    chain: chain::HashChain,
    current_state: Option<Value>,
}

impl<A: Actor> ActorRuntime<A> {
    pub fn new(actor: A) -> Result<Self> {
        let mut chain = chain::HashChain::new();
        chain.add(Value::Null); // Initialize chain with null entry

        Ok(Self {
            actor,
            chain,
            current_state: None,
        })
    }

    pub async fn init(&mut self) -> Result<()> {
        let initial_state = self.actor.init()?;
        self.current_state = Some(initial_state.clone());
        self.chain.add(initial_state);
        Ok(())
    }

    pub async fn handle_input(&mut self, input: ActorInput) -> Result<ActorOutput> {
        // Record the input in the chain
        let input_json = match &input {
            ActorInput::Message(msg) => serde_json::json!({
                "type": "message",
                "data": msg,
            }),
            ActorInput::HttpRequest {
                method,
                uri,
                headers,
                body,
            } => serde_json::json!({
                "type": "http-request",
                "data": {
                    "method": method,
                    "uri": uri,
                    "headers": headers,
                    "body": body.as_ref().map(|b| String::from_utf8_lossy(b).to_string()),
                }
            }),
        };
        self.chain.add(input_json);

        // Handle the input
        let current_state = self.current_state.as_ref().expect("State not initialized");
        let (output, new_state) = self.actor.handle_input(input, current_state)?;

        // Record the state change
        self.current_state = Some(new_state.clone());
        self.chain.add(new_state);

        Ok(output)
    }

    pub fn get_chain(&self) -> &chain::HashChain {
        &self.chain
    }
}

/// Trait for different ways of exposing actors to the world
pub trait ActorInterface {
    type Config;
    type ActorType: Actor;

    fn new(config: Self::Config) -> Result<Self>
    where
        Self: Sized;
    fn start(&mut self, runtime: ActorRuntime<Self::ActorType>) -> Result<()>;
}

pub struct Runtime<A: Actor> {
    core: ActorRuntime<A>,
    interfaces: Vec<Box<dyn ActorInterface<Config = (), ActorType = A>>>,
}

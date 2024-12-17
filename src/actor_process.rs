use crate::actor::Actor;
use crate::actor::ActorInput;
use crate::actor::ActorMessage;
use crate::actor::MessageMetadata;
use crate::chain::HashChain;
use crate::state::ActorState;
use crate::Result;
use serde_json::{json, Value};
use tokio::sync::mpsc;

pub struct ActorProcess {
    mailbox_rx: mpsc::Receiver<ActorMessage>,
    chain: HashChain,
    state: ActorState,
    actor: Box<dyn Actor>,
    name: String,
}

impl ActorProcess {
    pub fn new(
        name: &String,
        actor: Box<dyn Actor>,
        mailbox_rx: mpsc::Receiver<ActorMessage>,
    ) -> Result<Self> {
        let mut chain = HashChain::new();

        // Initialize actor state
        let initial_state = actor.init()?;
        let state = ActorState::new(initial_state.clone());

        // Record initialization in chain
        chain.add_event(
            "init".to_string(),
            json!({
                "initial_state": initial_state
            }),
        );

        Ok(Self {
            mailbox_rx,
            chain,
            state,
            actor,
            name: name.to_string(),
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        while let Some(msg) = self.mailbox_rx.recv().await {
            // Record received message in chain
            match &msg.metadata {
                Some(MessageMetadata::ActorSource {
                    source_actor,
                    source_chain_state,
                }) => {
                    self.chain.add_event("actor_message".to_string(), {
                        let content_value = match &msg.content {
                            ActorInput::Message(v) => v,
                            _ => &serde_json::to_value(&msg.content).unwrap_or_default(),
                        };
                        json!({
                            "source_actor": source_actor,
                            "source_chain_state": source_chain_state,
                            "content": content_value
                        })
                    });
                }
                _ => {
                    self.chain.add_event(
                        "external_input".to_string(),
                        json!({
                            "input": msg.content
                        }),
                    );
                }
            }

            // Process input using current state
            let current_state = self.state.get_state();
            let (output, new_state) = self.actor.handle_input(msg.content, current_state)?;

            // Verify and update state
            if self.actor.verify_state(&new_state) {
                let old_state = self.state.update_state(new_state);

                // Record state change in chain
                self.chain.add_event(
                    "state_change".to_string(),
                    json!({
                        "old_state": old_state,
                        "new_state": self.state.get_state()
                    }),
                );

                // Record output in chain
                self.chain.add_event(
                    "output".to_string(),
                    json!({
                        "output": output
                    }),
                );

                // Send response if metadata contains response channel
                if let Some(MessageMetadata::HttpRequest { response_channel }) = msg.metadata {
                    let _ = response_channel.send(output);
                }
            } else {
                // Record invalid state transition
                self.chain.add_event(
                    "invalid_state_transition".to_string(),
                    json!({
                        "attempted_state": new_state
                    }),
                );
            }
        }

        Ok(())
    }

    pub fn get_chain(&self) -> &HashChain {
        &self.chain
    }

    pub fn send_message(&mut self, target: &str, msg: Value) -> Result<()> {
        // Record the send event in chain
        self.chain.add_event(
            "message_sent".to_string(),
            json!({
                "target": target,
                "message": msg
            }),
        );

        // TODO: Implement actual message sending
        Ok(())
    }
}

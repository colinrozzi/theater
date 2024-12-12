use anyhow::Result;
use serde_json::json;
use theater::{Actor, ActorProcess, ActorInput, ActorOutput, ActorMessage};
use tokio::sync::mpsc;

struct TestActor;

impl Actor for TestActor {
    fn init(&self) -> Result<serde_json::Value> {
        Ok(json!({"count": 0}))
    }

    fn handle_input(
        &self,
        input: ActorInput,
        state: &serde_json::Value,
    ) -> Result<(ActorOutput, serde_json::Value)> {
        match input {
            ActorInput::Message(msg) => {
                let count = state["count"].as_i64().unwrap_or(0) + 1;
                let new_state = json!({"count": count});
                Ok((ActorOutput::Message(msg), new_state))
            }
            _ => Ok((
                ActorOutput::Message(json!({"error": "unsupported input"})),
                state.clone(),
            )),
        }
    }

    fn verify_state(&self, state: &serde_json::Value) -> bool {
        state.get("count").is_some()
    }
}

#[tokio::test]
async fn test_actor_process() -> Result<()> {
    let (tx, rx) = mpsc::channel(32);
    let actor = Box::new(TestActor);
    let mut process = ActorProcess::new(actor, rx)?;
    
    // Verify initial state
    assert!(process.get_chain().get_head().is_some());
    
    // Send test message
    let test_msg = ActorInput::Message(json!({"test": "message"}));
    tx.send(ActorMessage {
        content: test_msg,
        response_channel: None,
    })
    .await?;
    
    Ok(())
}

#[test]
fn test_actor_state_verification() {
    let actor = TestActor;
    
    // Valid state
    let valid_state = json!({"count": 0});
    assert!(actor.verify_state(&valid_state));
    
    // Invalid state
    let invalid_state = json!({"invalid": "state"});
    assert!(!actor.verify_state(&invalid_state));
}

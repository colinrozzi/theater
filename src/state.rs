use anyhow::Result;
use serde_json::Value;

/// Manages the current state of an actor
#[derive(Debug)]
pub struct ActorState {
    /// The current state value
    current_state: Value,
}

impl ActorState {
    /// Creates a new ActorState with an initial state value
    pub fn new(initial_state: Value) -> Self {
        Self {
            current_state: initial_state,
        }
    }

    /// Gets the current state
    pub fn get_state(&self) -> &Value {
        &self.current_state
    }

    /// Updates the current state
    /// Returns the old state
    pub fn update_state(&mut self, new_state: Value) -> Value {
        std::mem::replace(&mut self.current_state, new_state)
    }

    /// Verifies if a state transition is valid
    pub fn verify_transition(&self, new_state: &Value) -> bool {
        // For now, all transitions are considered valid
        // This could be expanded to include validation logic
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_state_management() {
        let initial = json!({"count": 0});
        let mut state = ActorState::new(initial);

        // Test get_state
        assert_eq!(state.get_state(), &json!({"count": 0}));

        // Test update_state
        let old_state = state.update_state(json!({"count": 1}));
        assert_eq!(old_state, json!({"count": 0}));
        assert_eq!(state.get_state(), &json!({"count": 1}));

        // Test verify_transition
        assert!(state.verify_transition(&json!({"count": 2})));
    }
}
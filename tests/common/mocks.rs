use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use theater::actor_executor::ActorOperation;
use tokio::sync::mpsc;

/// Mock actor handle for testing
pub struct MockActorHandle {
    operation_tx: mpsc::Sender<ActorOperation>,
    response_map: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl MockActorHandle {
    pub fn new() -> (Self, mpsc::Receiver<ActorOperation>) {
        let (tx, rx) = mpsc::channel(10);
        (
            Self {
                operation_tx: tx,
                response_map: Arc::new(Mutex::new(HashMap::new())),
            },
            rx,
        )
    }

    pub fn with_response(self, operation: &str, response: Vec<u8>) -> Self {
        self.response_map
            .lock()
            .unwrap()
            .insert(operation.to_string(), response);
        self
    }

    pub fn get_sender(&self) -> mpsc::Sender<ActorOperation> {
        self.operation_tx.clone()
    }
}

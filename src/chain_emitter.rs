use std::collections::VecDeque;
use std::sync::Mutex;
use chrono::Utc;

use crate::logging::ChainEvent;

pub struct ChainEmitter {
    history: Mutex<VecDeque<ChainEvent>>,
    max_history: usize,
}

impl ChainEmitter {
    pub fn new(max_history: usize) -> Self {
        Self {
            history: Mutex::new(VecDeque::with_capacity(max_history)),
            max_history,
        }
    }

    pub fn emit(&self, event: ChainEvent) {
        // Store in history
        let mut history = self.history.lock().unwrap();
        if history.len() >= self.max_history {
            history.pop_front();
        }
        history.push_back(event.clone());

        // Print to stdout in a clearly marked format
        println!("\n[CHAIN] Event at {}", Utc::now().to_rfc3339());
        println!("{}", event);
    }

    pub fn get_history(&self) -> Vec<ChainEvent> {
        self.history.lock().unwrap().iter().cloned().collect()
    }
}

// Global instance
lazy_static::lazy_static! {
    pub static ref CHAIN_EMITTER: ChainEmitter = ChainEmitter::new(1000);
}
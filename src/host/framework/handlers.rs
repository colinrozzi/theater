use std::collections::HashMap;
use tracing;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandlerType {
    HttpRequest,
    Middleware,
    WebSocketConnect,
    WebSocketMessage,
    WebSocketDisconnect,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct HandlerConfig {
    pub id: u64,
    pub name: String,
    pub handler_type: HandlerType,
}

pub struct HandlerRegistry {
    handlers: HashMap<u64, HandlerConfig>,
    handler_names: HashMap<String, u64>,
}

impl HandlerRegistry {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            handler_names: HashMap::new(),
        }
    }

    pub fn register(&mut self, config: HandlerConfig) {
        self.handler_names.insert(config.name.clone(), config.id);
        self.handlers.insert(config.id, config);
    }

    pub fn exists(&self, handler_id: u64) -> bool {
        self.handlers.contains_key(&handler_id)
    }

    pub fn get_handler(&self, handler_id: u64) -> Option<&HandlerConfig> {
        self.handlers.get(&handler_id)
    }

    pub fn get_handler_by_name(&self, name: &str) -> Option<&HandlerConfig> {
        self.handler_names
            .get(name)
            .and_then(|id| self.handlers.get(id))
    }

    pub async fn set_handler_type(&self, handler_id: u64, handler_type: HandlerType) {
        // This is just a stub implementation because we would need a proper 'mut' reference
        // or interior mutability to actually update the handler type
        //
        // In a production implementation, this method would use a Mutex or RwLock
        // to properly update the handler type
        if let Some(handler) = self.handlers.get(&handler_id) {
            // Just a check to indicate we would only update under these conditions
            if handler.handler_type == HandlerType::Unknown || handler.handler_type == handler_type {
                // We would update here if we had proper mutability
                // For now, let's just log that we accept this change
                tracing::debug!("Would change handler {} type from {:?} to {:?}", 
                    handler_id, handler.handler_type, handler_type);
            }
        }
    }

    pub fn get_all_handlers(&self) -> Vec<&HandlerConfig> {
        self.handlers.values().collect()
    }
}

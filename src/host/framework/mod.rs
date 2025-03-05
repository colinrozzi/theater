mod handlers;
mod http_framework;
mod server_instance;
mod types;

pub use http_framework::HttpFramework;
pub use handlers::{HandlerConfig, HandlerRegistry, HandlerType};
pub use types::*;

// Update event data in the events/http.rs file to include new event types
use crate::events::http::HttpEventData;

// Re-export for usage in other modules
pub type HttpFrameworkHandler = HttpFramework;

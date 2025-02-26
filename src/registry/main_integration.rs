// Example of how to integrate the registry in your main.rs
// Replace your existing main.rs with this structure or adapt as needed

use crate::error::Result;

mod cli;
mod error;
mod runtime;
mod config;
mod registry; // Add this line to include the registry module

// You may need to update this based on your actual structure
fn main() -> Result<()> {
    // Initialize logging
    env_logger::init_from_env(
        env_logger::Env::default().default_filter_or("info")
    );
    
    // Use the CLI module that supports registry commands
    cli::run()
}

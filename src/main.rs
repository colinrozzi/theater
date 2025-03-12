use anyhow::Result;

// Import the CLI module
mod cli;

fn main() -> Result<()> {
    // Run the CLI
    cli::run()
}

pub mod open;

use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct ChannelArgs {
    #[command(subcommand)]
    pub command: ChannelCommands,
}

#[derive(Debug, Subcommand)]
pub enum ChannelCommands {
    /// Open an interactive channel session with an actor
    #[command(name = "open")]
    Open(open::OpenArgs),
}

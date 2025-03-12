pub mod commands;
pub mod client;
pub mod templates;

use clap::{Parser, Subcommand};


/// Theater CLI - A WebAssembly actor system that enables state management,
/// verification, and flexible interaction patterns.
#[derive(Debug, Parser)]
#[command(name = "theater")]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Turn on verbose output
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Display output in JSON format
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Start a Theater server
    #[command(name = "server")]
    Server(commands::server::start::StartArgs),

    /// Create a new Theater actor project
    #[command(name = "create")]
    Create(commands::create::CreateArgs),

    /// Deploy an actor to a Theater server
    #[command(name = "deploy")]
    Deploy(commands::deploy::DeployArgs),

    /// List all running actors
    #[command(name = "list")]
    List(commands::list::ListArgs),

    /// View actor logs
    #[command(name = "logs")]
    Logs(commands::logs::LogsArgs),

    /// Get actor state
    #[command(name = "state")]
    State(commands::state::StateArgs),

    /// Get actor events
    #[command(name = "events")]
    Events(commands::events::EventsArgs),

    /// Start an actor from a manifest
    #[command(name = "start")]
    Start(commands::start::StartArgs),

    /// Stop a running actor
    #[command(name = "stop")]
    Stop(commands::stop::StopArgs),

    /// Restart a running actor
    #[command(name = "restart")]
    Restart(commands::restart::RestartArgs),

    /// Send a message to an actor
    #[command(name = "message")]
    Message(commands::message::MessageArgs),

    /// Watch a directory and redeploy actor on changes
    #[command(name = "watch")]
    Watch(commands::watch::WatchArgs),
}



/// Run the Theater CLI
pub fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    
    match &cli.command {
        Commands::Server(args) => {
            commands::server::start::execute(args, cli.verbose)
        }
        Commands::Create(args) => commands::create::execute(args, cli.verbose, cli.json),
        Commands::Deploy(args) => commands::deploy::execute(args, cli.verbose, cli.json),
        Commands::List(args) => commands::list::execute(args, cli.verbose, cli.json),
        Commands::Logs(args) => commands::logs::execute(args, cli.verbose, cli.json),
        Commands::State(args) => commands::state::execute(args, cli.verbose, cli.json),
        Commands::Events(args) => commands::events::execute(args, cli.verbose, cli.json),
        Commands::Start(args) => commands::start::execute(args, cli.verbose, cli.json),
        Commands::Stop(args) => commands::stop::execute(args, cli.verbose, cli.json),
        Commands::Restart(args) => commands::restart::execute(args, cli.verbose, cli.json),
        Commands::Message(args) => commands::message::execute(args, cli.verbose, cli.json),
        Commands::Watch(args) => commands::watch::execute(args, cli.verbose, cli.json),
    }
}

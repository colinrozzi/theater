pub mod client;
pub mod commands;
pub mod templates;
pub mod utils;

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

    /// Build a Theater actor to WebAssembly
    #[command(name = "build")]
    Build(commands::build::BuildArgs),

    /// Start or deploy an actor from a manifest
    #[command(name = "start")]
    Start(commands::start::StartArgs),

    /// Subscribe to real-time events from an actor
    #[command(name = "subscribe")]
    Subscribe(commands::subscribe::SubscribeArgs),

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

    /// Inspect a running actor (detailed view)
    #[command(name = "inspect")]
    Inspect(commands::inspect::InspectArgs),

    /// Show actor hierarchy as a tree
    #[command(name = "tree")]
    Tree(commands::tree::TreeArgs),

    /// Validate an actor manifest
    #[command(name = "validate")]
    Validate(commands::validate::ValidateArgs),

    /// Start interactive shell
    #[command(name = "shell")]
    Shell(commands::shell::ShellArgs),

    /// Stop a running actor
    #[command(name = "stop")]
    Stop(commands::stop::StopArgs),

    /// Restart a running actor
    #[command(name = "restart")]
    Restart(commands::restart::RestartArgs),

    /// Update an actor's component
    #[command(name = "update")]
    Update(commands::update::UpdateArgs),

    /// Send a message to an actor
    #[command(name = "message")]
    Message(commands::message::MessageArgs),

    /// Watch a directory and redeploy actor on changes
    #[command(name = "watch")]
    Watch(commands::watch::WatchArgs),

    /// Channel operations
    #[command(name = "channel")]
    Channel(commands::channel::ChannelArgs),
}

/// Run the Theater CLI
pub fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Subscribe(args) => commands::subscribe::execute(args, cli.verbose, cli.json),
        Commands::Server(args) => commands::server::start::execute(args, cli.verbose),
        Commands::Create(args) => commands::create::execute(args, cli.verbose, cli.json),
        Commands::Build(args) => commands::build::execute(args, cli.verbose, cli.json),
        Commands::List(args) => commands::list::execute(args, cli.verbose, cli.json),
        Commands::Logs(args) => commands::logs::execute(args, cli.verbose, cli.json),
        Commands::State(args) => commands::state::execute(args, cli.verbose, cli.json),
        Commands::Events(args) => commands::events::execute(args, cli.verbose, cli.json),
        Commands::Inspect(args) => commands::inspect::execute(args, cli.verbose, cli.json),
        Commands::Tree(args) => commands::tree::execute(args, cli.verbose, cli.json),
        Commands::Validate(args) => commands::validate::execute(args, cli.verbose, cli.json),
        Commands::Shell(args) => commands::shell::execute(args, cli.verbose, cli.json),
        Commands::Start(args) => commands::start::execute(args, cli.verbose, cli.json),
        Commands::Stop(args) => commands::stop::execute(args, cli.verbose, cli.json),
        Commands::Restart(args) => commands::restart::execute(args, cli.verbose, cli.json),
        Commands::Update(args) => commands::update::execute(args, cli.verbose, cli.json),
        Commands::Message(args) => commands::message::execute(args, cli.verbose, cli.json),
        Commands::Watch(args) => commands::watch::execute(args, cli.verbose, cli.json),
        Commands::Channel(args) => match &args.command {
            commands::channel::ChannelCommands::Open(open_args) => {
                commands::channel::open::execute(open_args, cli.verbose, cli.json)
            }
        },
    }
}

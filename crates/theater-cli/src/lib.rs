pub mod client;
pub mod commands;
pub mod config;
pub mod error;
pub mod output;
pub mod templates;
pub mod tui;
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

    /// Get actor state
    #[command(name = "state")]
    State(commands::state::StateArgs),

    /// Get actor events (from running actor or filesystem)
    #[command(name = "events")]
    Events(commands::events::EventsArgs),

    /// Interactively explore actor events with TUI
    #[command(name = "events-explore")]
    EventsExplore(commands::events_explore::ExploreArgs),

    /// Inspect a running actor (detailed view)
    #[command(name = "inspect")]
    Inspect(commands::inspect::InspectArgs),

    /// Stop a running actor
    #[command(name = "stop")]
    Stop(commands::stop::StopArgs),

    /// Send a message to an actor
    #[command(name = "message")]
    Message(commands::message::MessageArgs),

    /// List stored actor IDs
    #[command(name = "list-stored")]
    ListStored(commands::list_stored::ListStoredArgs),

    /// Channel operations
    #[command(name = "channel")]
    Channel(commands::channel::ChannelArgs),

    /// Generate shell completion scripts
    #[command(name = "completion")]
    Completion(commands::completion::CompletionArgs),

    /// Generate dynamic completions (internal use)
    #[command(name = "dynamic-completion", hide = true)]
    DynamicCompletion(commands::dynamic_completion::DynamicCompletionArgs),
}

/// Run the Theater CLI asynchronously with cancellation support
pub async fn run(
    cli: Cli,
    config: config::Config,
    shutdown_token: tokio_util::sync::CancellationToken,
) -> anyhow::Result<()> {
    // Create output manager
    let output = output::OutputManager::new(config.output.clone());

    // Create a context that contains shared resources
    let ctx = CommandContext {
        config,
        output,
        verbose: cli.verbose,
        json: cli.json,
        shutdown_token: shutdown_token.clone(),
    };

    // Execute the command with cancellation support
    let command_future = async {
        match &cli.command {
            Commands::Subscribe(args) => commands::subscribe::execute_async(args, &ctx)
                .await
                .map_err(|e| anyhow::Error::from(e)),
            Commands::Create(args) => commands::create::execute_async(args, &ctx)
                .await
                .map_err(|e| anyhow::Error::from(e)),
            Commands::Build(args) => commands::build::execute_async(args, &ctx)
                .await
                .map_err(|e| anyhow::Error::from(e)),
            Commands::List(args) => commands::list::execute_async(args, &ctx)
                .await
                .map_err(|e| anyhow::Error::from(e)),
            Commands::State(args) => commands::state::execute_async(args, &ctx)
                .await
                .map_err(|e| anyhow::Error::from(e)),
            Commands::Events(args) => commands::events::execute_async(args, &ctx)
                .await
                .map_err(|e| anyhow::Error::from(e)),
            Commands::EventsExplore(args) => commands::events_explore::execute_async(args, &ctx)
                .await
                .map_err(|e| anyhow::Error::from(e)),
            Commands::Inspect(args) => commands::inspect::execute_async(args, &ctx)
                .await
                .map_err(|e| anyhow::Error::from(e)),
            Commands::Start(args) => commands::start::execute_async(args, &ctx)
                .await
                .map_err(|e| anyhow::Error::from(e)),
            Commands::Stop(args) => commands::stop::execute_async(args, &ctx)
                .await
                .map_err(|e| anyhow::Error::from(e)),
            Commands::Message(args) => commands::message::execute_async(args, &ctx)
                .await
                .map_err(|e| anyhow::Error::from(e)),
            Commands::Channel(args) => match &args.command {
                commands::channel::ChannelCommands::Open(open_args) => {
                    commands::channel::open::execute_async(open_args, &ctx)
                        .await
                        .map_err(|e| anyhow::Error::from(e))
                }
            },
            Commands::ListStored(args) => commands::list_stored::execute_async(args, &ctx)
                .await
                .map_err(|e| anyhow::Error::from(e)),
            Commands::Completion(args) => commands::completion::execute_async(args, &ctx)
                .await
                .map_err(|e| anyhow::Error::from(e)),
            Commands::DynamicCompletion(args) => {
                commands::dynamic_completion::execute_async(args, &ctx)
                    .await
                    .map_err(|e| anyhow::Error::from(e))
            }
        }
    };

    // Race the command execution against cancellation
    let result = tokio::select! {
        result = command_future => result,
        _ = shutdown_token.cancelled() => {
            return Err(anyhow::anyhow!("Operation cancelled"));
        }
    };

    // Handle the result
    match result {
        Ok(()) => Ok(()),
        Err(e) => {
            // Use our enhanced error handling
            if let Some(cli_error) = e.downcast_ref::<error::CliError>() {
                ctx.output.error(&cli_error.user_message())?;
                if ctx.verbose {
                    eprintln!("\nDebug info: {:?}", cli_error);
                }
            } else {
                ctx.output.error(&format!("Error: {}", e))?;
                if ctx.verbose {
                    eprintln!("\nDebug info: {:?}", e);
                }
            }
            std::process::exit(1);
        }
    }
}

/// Shared context for command execution
pub struct CommandContext {
    pub config: config::Config,
    pub output: output::OutputManager,
    pub verbose: bool,
    pub json: bool,
    pub shutdown_token: tokio_util::sync::CancellationToken,
}

impl CommandContext {
    /// Create a theater client using the configured server address
    pub fn create_client(&self) -> client::TheaterClient {
        client::TheaterClient::new(self.config.server.default_address, self.shutdown_token.clone())
    }

    /// Get the server address from config or override
    pub fn server_address(
        &self,
        override_addr: Option<std::net::SocketAddr>,
    ) -> std::net::SocketAddr {
        override_addr.unwrap_or(self.config.server.default_address)
    }
}

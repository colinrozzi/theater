pub mod commands;
pub mod config;
pub mod error;
pub mod output;
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
    /// Create a new Theater actor project
    #[command(name = "create")]
    Create(commands::create::CreateArgs),

    /// Build a Theater actor to WebAssembly
    #[command(name = "build")]
    Build(commands::build::BuildArgs),

    /// Start an actor with a local runtime
    #[command(name = "start")]
    Start(commands::start::StartArgs),

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
            Commands::Create(args) => commands::create::execute_async(args, &ctx)
                .await
                .map_err(|e| anyhow::Error::from(e)),
            Commands::Build(args) => commands::build::execute_async(args, &ctx)
                .await
                .map_err(|e| anyhow::Error::from(e)),
            Commands::Start(args) => commands::start::execute_async(args, &ctx)
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

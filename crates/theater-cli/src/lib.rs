pub mod commands;
pub mod config;
pub mod error;
pub mod output;
pub mod templates;
pub mod utils;

use clap::{Parser, Subcommand, ValueEnum};

/// Log level for runtime/system logs
#[derive(Debug, Clone, Copy, ValueEnum, Default)]
pub enum LogLevel {
    /// Show error logs only
    Error,
    /// Show warning and error logs
    #[default]
    Warn,
    /// Show info, warning, and error logs
    Info,
    /// Show debug and above
    Debug,
    /// Show all logs including trace
    Trace,
}

impl From<LogLevel> for tracing::Level {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Error => tracing::Level::ERROR,
            LogLevel::Warn => tracing::Level::WARN,
            LogLevel::Info => tracing::Level::INFO,
            LogLevel::Debug => tracing::Level::DEBUG,
            LogLevel::Trace => tracing::Level::TRACE,
        }
    }
}

/// Theater CLI - A WebAssembly actor system that enables state management,
/// verification, and flexible interaction patterns.
#[derive(Debug, Parser)]
#[command(name = "theater")]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Set the log level for runtime/system logs
    #[arg(short, long, global = true, value_enum, default_value = "warn")]
    pub log_level: LogLevel,

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

    /// Compose a prebuilt actor member into a self-contained composite
    /// (member + bundled allocator). For crane / cargo-workspace builds
    /// where the member is already built and `theater build` (which re-runs
    /// cargo and assumes a standalone crate) doesn't fit.
    #[command(name = "compose")]
    Compose(commands::compose::ComposeArgs),

    /// Spawn an actor with a local runtime (setup + init)
    #[command(name = "spawn")]
    Spawn(commands::spawn::SpawnArgs),

    /// Set up an actor with a local runtime, but do not call its init
    /// export. The replay path uses this; otherwise drive init yourself.
    #[command(name = "setup")]
    Setup(commands::setup::SetupArgs),

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
        log_level: cli.log_level,
        json: cli.json,
        shutdown_token: shutdown_token.clone(),
    };

    // Execute the command with cancellation support
    let command_future = async {
        match &cli.command {
            Commands::Create(args) => commands::create::execute_async(args, &ctx)
                .await
                .map_err(anyhow::Error::from),
            Commands::Build(args) => commands::build::execute_async(args, &ctx)
                .await
                .map_err(anyhow::Error::from),
            Commands::Compose(args) => commands::compose::execute_compose(args, &ctx)
                .await
                .map_err(anyhow::Error::from),
            Commands::Spawn(args) => commands::spawn::execute_spawn(args, &ctx)
                .await
                .map_err(anyhow::Error::from),
            Commands::Setup(args) => commands::setup::execute_async(args, &ctx)
                .await
                .map_err(anyhow::Error::from),
            Commands::Completion(args) => commands::completion::execute_async(args, &ctx)
                .await
                .map_err(anyhow::Error::from),
            Commands::DynamicCompletion(args) => {
                commands::dynamic_completion::execute_async(args, &ctx)
                    .await
                    .map_err(anyhow::Error::from)
            }
        }
    };

    // Race the command execution against cancellation
    let result = tokio::select! {
        result = command_future => result,
        _ = shutdown_token.cancelled() => {
            return Ok(());
        }
    };

    // Handle the result
    match result {
        Ok(()) => Ok(()),
        Err(e) => {
            // Use our enhanced error handling
            if let Some(cli_error) = e.downcast_ref::<error::CliError>() {
                ctx.output.error(&cli_error.user_message())?;
                if ctx.is_verbose() {
                    eprintln!("\nDebug info: {:?}", cli_error);
                }
            } else {
                ctx.output.error(&format!("Error: {}", e))?;
                if ctx.is_verbose() {
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
    pub log_level: LogLevel,
    pub json: bool,
    pub shutdown_token: tokio_util::sync::CancellationToken,
}

impl CommandContext {
    /// Returns true if log level is debug or higher (more verbose)
    pub fn is_verbose(&self) -> bool {
        matches!(self.log_level, LogLevel::Debug | LogLevel::Trace)
    }
}

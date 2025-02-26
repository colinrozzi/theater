use anyhow::Result;
use console::style;
use tracing::info;
use theater::cli::{Args, Commands};
use theater::cli::{actor, manifest, system, dev};
use clap::Parser;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Setup logging based on verbosity
    if args.verbose {
        tracing_subscriber::fmt()
            .with_env_filter("debug")
            .with_line_number(true)
            .with_file(true)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter("info")
            .with_target(false)
            .with_line_number(false)
            .with_file(false)
            .init();
    }

    match args.command {
        // Legacy commands with deprecation notices
        Commands::Start { manifest, address } => {
            println!("{} The 'start' command is deprecated. Use 'theater actor start' instead.", 
                style("Warning:").yellow().bold()
            );
            
            theater::cli::legacy::execute_command(
                theater::cli::legacy::Commands::Start { manifest },
                &address
            ).await?;
        },
        Commands::Stop { id, address } => {
            println!("{} The 'stop' command is deprecated. Use 'theater actor stop' instead.", 
                style("Warning:").yellow().bold()
            );
            
            theater::cli::legacy::execute_command(
                theater::cli::legacy::Commands::Stop { id },
                &address
            ).await?;
        },
        Commands::List { detailed, address } => {
            println!("{} The 'list' command is deprecated. Use 'theater actor list' instead.", 
                style("Warning:").yellow().bold()
            );
            
            theater::cli::legacy::execute_command(
                theater::cli::legacy::Commands::List { detailed },
                &address
            ).await?;
        },
        Commands::Subscribe { id, address } => {
            println!("{} The 'subscribe' command is deprecated. Use 'theater actor subscribe' instead.", 
                style("Warning:").yellow().bold()
            );
            
            theater::cli::legacy::execute_command(
                theater::cli::legacy::Commands::Subscribe { id },
                &address
            ).await?;
        },
        Commands::Interactive { address } => {
            println!("{} The 'interactive' command is deprecated. Use the new specific commands instead.", 
                style("Warning:").yellow().bold()
            );
            
            theater::cli::legacy::run_interactive_mode(&address).await?;
        },
        
        // New commands
        Commands::Manifest(cmd) => {
            let args = manifest::ManifestArgs { command: cmd };
            manifest::handle_manifest_command(args).await?;
        },
        Commands::Actor(cmd) => {
            let args = actor::ActorArgs { 
                address: "127.0.0.1:9000".to_string(), 
                command: cmd 
            };
            actor::handle_actor_command(args).await?;
        },
        Commands::System(cmd) => {
            let args = system::SystemArgs { 
                address: "127.0.0.1:9000".to_string(), 
                command: cmd 
            };
            system::handle_system_command(args).await?;
        },
        Commands::Dev(cmd) => {
            let args = dev::DevArgs { command: cmd };
            dev::handle_dev_command(args).await?;
        },
    }

    Ok(())
}

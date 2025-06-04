use clap::Parser;
use std::net::SocketAddr;

/// Theater Server CLI - Start and manage a Theater WebAssembly actor system server
#[derive(Debug, Parser)]
#[command(name = "theater-server")]
#[command(author, version, about)]
pub struct ServerArgs {
    /// Address to bind the theater server to
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    pub address: SocketAddr,

    /// Logging level (simple version, e.g. 'info', 'debug')
    #[arg(short, long, default_value = "info")]
    pub log_level: String,

    /// Advanced logging filter (e.g. "theater=debug,wasmtime=info")
    /// This overrides log_level if provided
    #[arg(long)]
    pub log_filter: Option<String>,

    /// Log directory
    #[arg(long, default_value = "$THEATER_HOME/logs/theater")]
    pub log_dir: String,

    /// Log to stdout
    #[arg(long)]
    pub log_stdout: bool,
}

use anyhow::Result;
use clap::Parser;

mod args;
mod server;

use args::ServerArgs;

#[tokio::main]
async fn main() -> Result<()> {
    let args = ServerArgs::parse();
    server::start_server(&args).await
}

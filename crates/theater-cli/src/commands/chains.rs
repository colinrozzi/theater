use anyhow::Result;
use clap::{Parser, Subcommand};
use std::fs;
use std::path::PathBuf;

use crate::{error::CliError, CommandContext};
use theater::chain::ChainWriter;

/// Default local chains directory
const LOCAL_CHAINS_DIR: &str = "chains";

#[derive(Debug, Parser)]
pub struct ChainsArgs {
    /// Use local ./chains/ directory only
    #[arg(short, long, conflicts_with = "global")]
    pub local: bool,

    /// Use global /tmp/theater/chains/ directory only
    #[arg(short, long, conflicts_with = "local")]
    pub global: bool,

    #[command(subcommand)]
    pub command: Option<ChainsCommand>,

    /// Chain ID to inspect (when no subcommand)
    #[arg(value_name = "ID")]
    pub id: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum ChainsCommand {
    /// Garbage collect old chains
    #[command(name = "gc")]
    Gc,

    /// Save a chain from global to local
    #[command(name = "save")]
    Save {
        /// Chain ID to save
        id: String,

        /// Name for the saved chain (default: actor-id)
        #[arg(value_name = "NAME")]
        name: Option<String>,
    },
}

pub async fn execute_async(args: &ChainsArgs, ctx: &CommandContext) -> Result<(), CliError> {
    match &args.command {
        Some(ChainsCommand::Gc) => execute_gc(args, ctx).await,
        Some(ChainsCommand::Save { id, name }) => execute_save(id, name.as_deref(), ctx).await,
        None => {
            if let Some(id) = &args.id {
                execute_inspect(id, args, ctx).await
            } else {
                execute_list(args, ctx).await
            }
        }
    }
}

/// List chains
async fn execute_list(args: &ChainsArgs, _ctx: &CommandContext) -> Result<(), CliError> {
    let local_dir = PathBuf::from(LOCAL_CHAINS_DIR);
    let global_dir = ChainWriter::chains_dir();

    let mut found_any = false;

    // Check local first (unless --global specified)
    if !args.global {
        if local_dir.exists() {
            let chains = list_chains_in_dir(&local_dir)?;
            if !chains.is_empty() {
                println!("Local chains ({}):", local_dir.display());
                for chain in &chains {
                    print_chain_summary(chain)?;
                }
                found_any = true;
            }
        }
    }

    // Check global (unless --local specified)
    if !args.local {
        if global_dir.exists() {
            let chains = list_chains_in_dir(&global_dir)?;
            if !chains.is_empty() {
                if found_any {
                    println!();
                }
                println!("Global chains ({}):", global_dir.display());
                for chain in &chains {
                    print_chain_summary(chain)?;
                }
                found_any = true;
            }
        }
    }

    if !found_any {
        println!("No chains found.");
    }

    Ok(())
}

/// Inspect a specific chain
async fn execute_inspect(
    id: &str,
    args: &ChainsArgs,
    _ctx: &CommandContext,
) -> Result<(), CliError> {
    let chain_path = find_chain(id, args)?;

    println!("Chain: {}", chain_path.display());
    println!();

    // Read and parse the chain file
    let contents = fs::read_to_string(&chain_path).map_err(|e| {
        CliError::file_operation_failed("read chain", chain_path.display().to_string(), e)
    })?;

    let mut event_count = 0;
    let mut event_types: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    // Parse events (simple parsing of EVENT format)
    for line in contents.lines() {
        if line.starts_with("EVENT ") {
            event_count += 1;
        } else if !line.is_empty()
            && !line.starts_with("0000000000000000")
            && !line.chars().all(|c| c.is_ascii_hexdigit())
            && !line.chars().all(|c| c.is_ascii_digit())
        {
            // This is likely an event type line
            if !line.starts_with('{') && !line.contains(':') {
                // Skip, probably body content
            } else if !line.starts_with('{') {
                *event_types.entry(line.to_string()).or_insert(0) += 1;
            }
        }
    }

    let file_size = fs::metadata(&chain_path).map(|m| m.len()).unwrap_or(0);

    println!("Events: {}", event_count);
    println!("Size: {}", format_size(file_size));
    println!();

    if !event_types.is_empty() {
        println!("Event types:");
        let mut types: Vec<_> = event_types.into_iter().collect();
        types.sort_by(|a, b| b.1.cmp(&a.1));
        for (event_type, count) in types.iter().take(10) {
            println!("  {} ({})", event_type, count);
        }
        if types.len() > 10 {
            println!("  ... and {} more types", types.len() - 10);
        }
    }

    // Check for meta file
    let meta_path = chain_path.with_extension("meta.json");
    if meta_path.exists() {
        if let Ok(meta_contents) = fs::read_to_string(&meta_path) {
            println!();
            println!("Metadata:");
            println!("{}", meta_contents);
        }
    }

    Ok(())
}

/// Garbage collect chains
async fn execute_gc(args: &ChainsArgs, _ctx: &CommandContext) -> Result<(), CliError> {
    let dir = if args.local {
        PathBuf::from(LOCAL_CHAINS_DIR)
    } else {
        // Default to global for gc
        ChainWriter::chains_dir()
    };

    if !dir.exists() {
        println!("No chains directory found at {}", dir.display());
        return Ok(());
    }

    let chains = list_chains_in_dir(&dir)?;
    if chains.is_empty() {
        println!("No chains to clean up.");
        return Ok(());
    }

    let mut total_size = 0u64;
    let mut count = 0;

    for chain_path in &chains {
        // Get size before deleting
        if let Ok(metadata) = fs::metadata(chain_path) {
            total_size += metadata.len();
        }

        // Delete chain file
        if let Err(e) = fs::remove_file(chain_path) {
            eprintln!("Failed to remove {}: {}", chain_path.display(), e);
            continue;
        }

        // Delete meta file if exists
        let meta_path = chain_path.with_extension("meta.json");
        if meta_path.exists() {
            if let Ok(metadata) = fs::metadata(&meta_path) {
                total_size += metadata.len();
            }
            let _ = fs::remove_file(&meta_path);
        }

        count += 1;
    }

    println!(
        "Removed {} chain(s), freed {}",
        count,
        format_size(total_size)
    );

    Ok(())
}

/// Save a chain from global to local
async fn execute_save(id: &str, name: Option<&str>, _ctx: &CommandContext) -> Result<(), CliError> {
    let global_dir = ChainWriter::chains_dir();
    let local_dir = PathBuf::from(LOCAL_CHAINS_DIR);

    // Find the chain in global
    let source_path = find_chain_in_dir(id, &global_dir)?.ok_or_else(|| {
        CliError::invalid_manifest(format!("Chain '{}' not found in global directory", id))
    })?;

    // Create local chains directory
    fs::create_dir_all(&local_dir).map_err(|e| {
        CliError::file_operation_failed("create directory", local_dir.display().to_string(), e)
    })?;

    // Determine destination name
    let dest_name = name.unwrap_or(id);
    let dest_path = local_dir.join(format!("{}.chain", dest_name));

    // Copy chain file
    fs::copy(&source_path, &dest_path).map_err(|e| {
        CliError::file_operation_failed("copy chain", dest_path.display().to_string(), e)
    })?;

    // Copy meta file if exists
    let source_meta = source_path.with_extension("meta.json");
    if source_meta.exists() {
        let dest_meta = dest_path.with_extension("meta.json");
        let _ = fs::copy(&source_meta, &dest_meta);
    }

    println!("Saved chain to {}", dest_path.display());

    Ok(())
}

/// List all .chain files in a directory
fn list_chains_in_dir(dir: &PathBuf) -> Result<Vec<PathBuf>, CliError> {
    let entries = fs::read_dir(dir).map_err(|e| {
        CliError::file_operation_failed("read directory", dir.display().to_string(), e)
    })?;

    let mut chains: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|e| e == "chain").unwrap_or(false))
        .collect();

    // Sort by modification time (newest first)
    chains.sort_by(|a, b| {
        let a_time = fs::metadata(a).and_then(|m| m.modified()).ok();
        let b_time = fs::metadata(b).and_then(|m| m.modified()).ok();
        b_time.cmp(&a_time)
    });

    Ok(chains)
}

/// Find a chain by ID (partial match supported)
fn find_chain(id: &str, args: &ChainsArgs) -> Result<PathBuf, CliError> {
    let local_dir = PathBuf::from(LOCAL_CHAINS_DIR);
    let global_dir = ChainWriter::chains_dir();

    // Check local first (unless --global)
    if !args.global {
        if let Some(path) = find_chain_in_dir(id, &local_dir)? {
            return Ok(path);
        }
    }

    // Check global (unless --local)
    if !args.local {
        if let Some(path) = find_chain_in_dir(id, &global_dir)? {
            return Ok(path);
        }
    }

    Err(CliError::invalid_manifest(format!(
        "Chain '{}' not found",
        id
    )))
}

/// Find a chain in a specific directory
fn find_chain_in_dir(id: &str, dir: &PathBuf) -> Result<Option<PathBuf>, CliError> {
    if !dir.exists() {
        return Ok(None);
    }

    // Try exact match first
    let exact_path = dir.join(format!("{}.chain", id));
    if exact_path.exists() {
        return Ok(Some(exact_path));
    }

    // Try partial match
    let entries = fs::read_dir(dir).map_err(|e| {
        CliError::file_operation_failed("read directory", dir.display().to_string(), e)
    })?;

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().map(|e| e == "chain").unwrap_or(false) {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if stem.starts_with(id) || stem.contains(id) {
                    return Ok(Some(path));
                }
            }
        }
    }

    Ok(None)
}

/// Print a summary of a chain file
fn print_chain_summary(path: &PathBuf) -> Result<(), CliError> {
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");

    let size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);

    // Try to get actor name from meta
    let meta_path = path.with_extension("meta.json");
    let actor_name = if meta_path.exists() {
        fs::read_to_string(&meta_path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v.get("actor_name")?.as_str().map(String::from))
    } else {
        None
    };

    if let Some(actor) = actor_name {
        println!("  {} ({}) - {}", name, actor, format_size(size));
    } else {
        println!("  {} - {}", name, format_size(size));
    }

    Ok(())
}

/// Format bytes as human-readable size
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} bytes", bytes)
    }
}

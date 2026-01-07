//! Replay command for verifying actor determinism
//!
//! This command replays an actor using a recorded event chain and verifies
//! that the actor produces the same chain hashes, proving deterministic behavior.

use clap::Parser;
use std::fs;
use std::path::Path;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tracing::debug;

use crate::error::{CliError, CliResult};
use crate::CommandContext;
use theater::chain::ChainEvent;
use theater::config::actor_manifest::RuntimeHostConfig;
use theater::handler::HandlerRegistry;
use theater::messages::TheaterCommand;
use theater::theater_runtime::TheaterRuntime;
use theater::utils::resolve_reference;
use theater::ActorError;
use theater_handler_runtime::RuntimeHandler;

/// Replay an actor and verify determinism against a recorded event chain
#[derive(Debug, Parser)]
pub struct ReplayArgs {
    /// Path to the actor manifest file
    #[arg(required = true)]
    pub manifest: String,

    /// Path to the recorded event chain file
    #[arg(required = true)]
    pub chain: String,

    /// Show verbose output during replay
    #[arg(short, long)]
    pub verbose: bool,

    /// Save the replay chain to this file for comparison
    #[arg(long)]
    pub save_replay: Option<String>,

    /// Timeout in seconds for the replay (default: 30)
    #[arg(long, default_value = "30")]
    pub timeout: u64,
}

/// Execute the replay command
pub async fn execute_async(args: &ReplayArgs, ctx: &CommandContext) -> CliResult<()> {
    debug!("Starting replay verification");
    debug!("Manifest: {}", args.manifest);
    debug!("Chain: {}", args.chain);

    // Load the recorded chain
    let chain_content = fs::read_to_string(&args.chain).map_err(|e| CliError::IoError {
        operation: format!("read chain file '{}'", args.chain),
        source: e,
    })?;

    let recorded_chain: Vec<ChainEvent> =
        serde_json::from_str(&chain_content).map_err(|e| CliError::ParseError {
            message: format!("Failed to parse chain file: {}", e),
        })?;

    if recorded_chain.is_empty() {
        return Err(CliError::InvalidInput {
            field: "chain".to_string(),
            value: args.chain.clone(),
            suggestion: "The chain file is empty. Record an actor run first.".to_string(),
        });
    }

    if args.verbose || ctx.verbose {
        ctx.output
            .info(&format!("Loaded chain with {} events", recorded_chain.len()))?;
    }

    // Load and modify the manifest to add replay handler
    let manifest_bytes = resolve_reference(&args.manifest).await.map_err(|e| {
        CliError::invalid_manifest(format!(
            "Failed to resolve manifest '{}': {}",
            args.manifest, e
        ))
    })?;

    let manifest_content = String::from_utf8(manifest_bytes).map_err(|e| {
        CliError::invalid_manifest(format!("Manifest content is not valid UTF-8: {}", e))
    })?;

    // Create replay manifest by adding replay handler config
    let chain_path = Path::new(&args.chain)
        .canonicalize()
        .map_err(|e| CliError::IoError {
            operation: format!("resolve chain path '{}'", args.chain),
            source: e,
        })?;

    let replay_manifest = create_replay_manifest(&manifest_content, &chain_path.display().to_string());

    if args.verbose || ctx.verbose {
        ctx.output.info("Starting replay...")?;
    }

    // Run the replay
    let replay_chain = run_replay(&replay_manifest, Duration::from_secs(args.timeout)).await?;

    if args.verbose || ctx.verbose {
        ctx.output
            .info(&format!("Replay produced {} events", replay_chain.len()))?;
    }

    // Save replay chain if requested
    if let Some(save_path) = &args.save_replay {
        let json = serde_json::to_string_pretty(&replay_chain).map_err(|e| CliError::ParseError {
            message: format!("Failed to serialize replay chain: {}", e),
        })?;
        fs::write(save_path, json).map_err(|e| CliError::IoError {
            operation: format!("write replay chain to '{}'", save_path),
            source: e,
        })?;
        if args.verbose || ctx.verbose {
            ctx.output
                .info(&format!("Saved replay chain to {}", save_path))?;
        }
    }

    // Compare chains
    let result = compare_chains(&recorded_chain, &replay_chain);

    // Print results
    print_comparison_results(&result, &recorded_chain, &replay_chain, args.verbose || ctx.verbose, ctx)?;

    if result.passed {
        ctx.output.success("Replay verification PASSED - actor is deterministic")?;
        Ok(())
    } else {
        Err(CliError::InvalidInput {
            field: "replay".to_string(),
            value: format!("{} mismatches", result.mismatches),
            suggestion: "The actor produced different results during replay. Check for non-deterministic operations.".to_string(),
        })
    }
}

/// Create a replay manifest by injecting the replay handler
fn create_replay_manifest(original_manifest: &str, chain_path: &str) -> String {
    // Parse the original manifest to find a good place to inject
    // For now, we'll just add the replay handler at the top of handlers

    // Check if there's already a [[handler]] section
    if original_manifest.contains("[[handler]]") {
        // Insert replay handler before the first [[handler]]
        let replay_handler = format!(
            r#"[[handler]]
type = "replay"
chain = "{}"

"#,
            chain_path
        );
        original_manifest.replacen("[[handler]]", &format!("{}[[handler]]", replay_handler), 1)
    } else {
        // Just append the replay handler
        format!(
            r#"{}

[[handler]]
type = "replay"
chain = "{}"
"#,
            original_manifest, chain_path
        )
    }
}

/// Run the replay and collect events
async fn run_replay(manifest: &str, timeout_duration: Duration) -> CliResult<Vec<ChainEvent>> {
    let (theater_tx, theater_rx) = mpsc::channel::<TheaterCommand>(32);

    // Create handler registry
    let mut registry = HandlerRegistry::new();
    let runtime_config = RuntimeHostConfig {};
    registry.register(RuntimeHandler::new(runtime_config, theater_tx.clone(), None));

    // Create runtime
    let mut runtime = TheaterRuntime::new(theater_tx.clone(), theater_rx, None, registry)
        .await
        .map_err(|e| CliError::ServerError {
            message: format!("Failed to create runtime: {}", e),
        })?;

    let runtime_handle = tokio::spawn(async move { runtime.run().await });

    // Create event subscription channel
    let (event_tx, mut event_rx) = mpsc::channel::<Result<ChainEvent, ActorError>>(100);

    // Spawn the actor
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    theater_tx
        .send(TheaterCommand::SpawnActor {
            manifest_path: manifest.to_string(),
            init_bytes: None,
            parent_id: None,
            response_tx,
            supervisor_tx: None,
            subscription_tx: Some(event_tx),
        })
        .await
        .map_err(|e| CliError::ServerError {
            message: format!("Failed to send spawn command: {}", e),
        })?;

    // Wait for spawn
    let spawn_result = timeout(Duration::from_secs(10), response_rx)
        .await
        .map_err(|_| CliError::operation_timeout("Actor spawn", 10))?
        .map_err(|e| CliError::ServerError {
            message: format!("Failed to receive spawn response: {}", e),
        })?
        .map_err(|e| CliError::ServerError {
            message: format!("Actor spawn failed: {}", e),
        })?;

    debug!("Replay actor started: {}", spawn_result);

    // Collect events
    let mut events = Vec::new();
    let start = std::time::Instant::now();
    let mut last_event_time = std::time::Instant::now();

    while start.elapsed() < timeout_duration {
        match timeout(Duration::from_millis(100), event_rx.recv()).await {
            Ok(Some(Ok(event))) => {
                last_event_time = std::time::Instant::now();
                events.push(event);
            }
            Ok(Some(Err(_))) | Ok(None) => break,
            Err(_) => {
                // Timeout on recv - check if we've been idle too long
                if last_event_time.elapsed() > Duration::from_secs(2) {
                    break;
                }
            }
        }
    }

    // Stop the actor
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    let _ = theater_tx
        .send(TheaterCommand::StopActor {
            actor_id: spawn_result,
            response_tx: stop_tx,
        })
        .await;
    let _ = timeout(Duration::from_secs(5), stop_rx).await;

    // Shutdown runtime
    drop(theater_tx);
    let _ = timeout(Duration::from_secs(5), runtime_handle).await;

    Ok(events)
}

/// Result of chain comparison
struct ComparisonResult {
    passed: bool,
    mismatches: usize,
    same_length: bool,
}

/// Compare two chains
fn compare_chains(original: &[ChainEvent], replay: &[ChainEvent]) -> ComparisonResult {
    let same_length = original.len() == replay.len();
    let mut mismatches = 0;

    let max_len = original.len().max(replay.len());
    for i in 0..max_len {
        let orig_hash = original.get(i).map(|e| &e.hash);
        let replay_hash = replay.get(i).map(|e| &e.hash);
        if orig_hash != replay_hash {
            mismatches += 1;
        }
    }

    ComparisonResult {
        passed: mismatches == 0 && same_length,
        mismatches,
        same_length,
    }
}

/// Print comparison results
fn print_comparison_results(
    result: &ComparisonResult,
    original: &[ChainEvent],
    replay: &[ChainEvent],
    verbose: bool,
    ctx: &CommandContext,
) -> CliResult<()> {
    ctx.output.info("")?;
    ctx.output.info("=== Replay Verification Results ===")?;
    ctx.output.info("")?;

    ctx.output
        .info(&format!("Original chain: {} events", original.len()))?;
    ctx.output
        .info(&format!("Replay chain:   {} events", replay.len()))?;

    if verbose {
        ctx.output.info("")?;
        ctx.output.info("Hash comparison:")?;

        let max_len = original.len().max(replay.len());
        for i in 0..max_len {
            let orig_hash = original
                .get(i)
                .map(|e| hex::encode(&e.hash[..8.min(e.hash.len())]))
                .unwrap_or_else(|| "-".to_string());
            let replay_hash = replay
                .get(i)
                .map(|e| hex::encode(&e.hash[..8.min(e.hash.len())]))
                .unwrap_or_else(|| "-".to_string());
            let event_type = original
                .get(i)
                .map(|e| e.event_type.as_str())
                .or_else(|| replay.get(i).map(|e| e.event_type.as_str()))
                .unwrap_or("-");

            let matches = original.get(i).map(|e| &e.hash) == replay.get(i).map(|e| &e.hash);
            let indicator = if matches { "OK" } else { "MISMATCH" };

            ctx.output.info(&format!(
                "  {}: {} vs {} [{}] {}",
                i, orig_hash, replay_hash, event_type, indicator
            ))?;
        }
    }

    ctx.output.info("")?;
    if result.same_length {
        ctx.output
            .info(&format!("Chain lengths match: {} events", original.len()))?;
    } else {
        ctx.output.info(&format!(
            "Chain lengths DIFFER: original={}, replay={}",
            original.len(),
            replay.len()
        ))?;
    }

    if result.mismatches == 0 {
        ctx.output.info("All hashes match")?;
    } else {
        ctx.output
            .info(&format!("{} hash mismatches found", result.mismatches))?;
    }

    ctx.output.info("")?;

    Ok(())
}

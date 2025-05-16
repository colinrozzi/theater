use anyhow::Result;
use clap::Parser;
use console::style;
use theater::id::TheaterId;
use std::str::FromStr;

#[derive(Debug, Parser)]
pub struct ListStoredArgs {
    /// Directory where chains are stored (defaults to THEATER_HOME/chains)
    #[arg(long)]
    pub chains_dir: Option<String>,
}

pub fn execute(args: &ListStoredArgs, _verbose: bool, json: bool) -> Result<()> {
    // Determine chains directory
    let theater_home = std::env::var("THEATER_HOME")
        .unwrap_or_else(|_| format!("{}/.theater", std::env::var("HOME").unwrap_or_default()));
    
    let chains_dir = args.chains_dir
        .clone()
        .unwrap_or_else(|| format!("{}/chains", theater_home));
    
    // Read the chains directory
    let path = std::path::Path::new(&chains_dir);
    
    if !path.exists() {
        if json {
            println!("{{\"actor_ids\": [], \"count\": 0}}");
        } else {
            println!("{} No stored actors found. Chains directory does not exist: {}",
                     style("ℹ").blue().bold(), chains_dir);
        }
        return Ok(());
    }
    
    // Collect actor IDs from filenames in the chains directory
    let mut actor_ids = Vec::new();
    
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let file_path = entry.path();
        
        if file_path.is_file() {
            if let Some(file_name) = file_path.file_name() {
                if let Some(name_str) = file_name.to_str() {
                    // Try to parse as TheaterId
                    if let Ok(actor_id) = TheaterId::from_str(name_str) {
                        actor_ids.push(actor_id);
                    }
                }
            }
        }
    }
    
    // Output results
    if json {
        let ids_json: Vec<String> = actor_ids.iter().map(|id| id.to_string()).collect();
        println!("{{\"actor_ids\": {}, \"count\": {}}}",
                 serde_json::to_string(&ids_json)?, actor_ids.len());
    } else {
        println!("{} Stored actors: {}",
                 style("ℹ").blue().bold(),
                 style(actor_ids.len().to_string()).cyan());
        
        if actor_ids.is_empty() {
            println!("  No stored actors found.");
        } else {
            for (i, actor_id) in actor_ids.iter().enumerate() {
                println!("  {}. {}", i + 1, actor_id);
            }
        }
    }
    
    Ok(())
}

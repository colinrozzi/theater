use anyhow::Result;
use clap::{Args, Subcommand};
use console::style;
use dialoguer::{Confirm, Select, Input, theme::ColorfulTheme};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::fs;
use tokio::time::sleep;

#[derive(Args)]
pub struct DevArgs {
    #[command(subcommand)]
    pub command: DevCommands,
}

#[derive(Subcommand)]
pub enum DevCommands {
    /// Create a new actor project with templates
    Scaffold {
        /// Name of the project
        #[arg(value_name = "NAME")]
        name: Option<String>,
        
        /// Directory to create the project in
        #[arg(short, long)]
        directory: Option<PathBuf>,
        
        /// Type of actor to create (http, message, websocket, etc.)
        #[arg(short, long)]
        actor_type: Option<String>,
        
        /// Programming language (rust, assemblyscript, etc.)
        #[arg(short, long)]
        language: Option<String>,
    },
    /// Build a WASM component from source
    Build {
        /// Path to the source directory or file
        #[arg(value_name = "PATH")]
        path: Option<PathBuf>,
        
        /// Output directory
        #[arg(short, long)]
        output: Option<PathBuf>,
        
        /// Release mode (optimized build)
        #[arg(short, long)]
        release: bool,
    },
    /// Test an actor locally
    Test {
        /// Path to the actor manifest
        #[arg(value_name = "MANIFEST")]
        manifest: PathBuf,
        
        /// Test script to run
        #[arg(short, long)]
        script: Option<PathBuf>,
        
        /// Verbose output
        #[arg(short, long)]
        verbose: bool,
    },
    /// Watch for changes and auto-restart actors
    Watch {
        /// Path to the actor manifest
        #[arg(value_name = "MANIFEST")]
        manifest: PathBuf,
        
        /// Directories to watch for changes
        #[arg(short, long)]
        watch_dirs: Option<Vec<PathBuf>>,
        
        /// Address of the theater server
        #[arg(short, long, default_value = "127.0.0.1:9000")]
        address: String,
    },
}

pub async fn handle_dev_command(args: DevArgs) -> Result<()> {
    match args.command {
        DevCommands::Scaffold { name, directory, actor_type, language } => {
            scaffold_project(name, directory, actor_type, language).await
        },
        DevCommands::Build { path, output, release } => {
            build_wasm_component(path, output, release).await
        },
        DevCommands::Test { manifest, script, verbose } => {
            test_actor_locally(manifest, script, verbose).await
        },
        DevCommands::Watch { manifest, watch_dirs, address } => {
            watch_and_restart(manifest, watch_dirs, address).await
        },
    }
}

async fn scaffold_project(
    name_opt: Option<String>,
    directory_opt: Option<PathBuf>,
    actor_type_opt: Option<String>,
    language_opt: Option<String>,
) -> Result<()> {
    println!("{}", style("Theater Actor Project Scaffolding").bold().cyan());
    
    // Get project name
    let name = match name_opt {
        Some(n) => n,
        None => {
            Input::with_theme(&ColorfulTheme::default())
                .with_prompt("Project name")
                .default("my-theater-actor".to_string())
                .interact()?
        }
    };
    
    // Get project directory
    let directory = match directory_opt {
        Some(d) => d,
        None => {
            let dir_str: String = Input::with_theme(&ColorfulTheme::default())
                .with_prompt("Project directory")
                .default(format!("./{}", name))
                .interact()?;
                
            PathBuf::from(dir_str)
        }
    };
    
    // Check if directory exists
    if directory.exists() {
        // Fixed: Using std::fs to check if directory is empty
        let is_empty = std::fs::read_dir(&directory)?.next().is_none();
        
        if !is_empty {
            let overwrite = Confirm::with_theme(&ColorfulTheme::default())
                .with_prompt(format!("Directory {} is not empty. Continue anyway?", directory.display()))
                .default(false)
                .interact()?;
                
            if !overwrite {
                println!("{} Project creation canceled", style("Canceled:").yellow().bold());
                return Ok(());
            }
        }
    } else {
        fs::create_dir_all(&directory).await?;
    }
    
    // Select language
    let languages = vec!["Rust", "AssemblyScript"];
    let language = match language_opt {
        Some(l) => l,
        None => {
            let selection = Select::with_theme(&ColorfulTheme::default())
                .with_prompt("Select programming language")
                .default(0)
                .items(&languages)
                .interact()?;
                
            languages[selection].to_string().to_lowercase()
        }
    };
    
    // Select actor type
    let actor_types = vec![
        "HTTP Server", 
        "Message Handler",
        "WebSocket Server", 
        "Basic Actor",
        "Supervisor"
    ];
    let actor_type = match actor_type_opt {
        Some(t) => t,
        None => {
            let selection = Select::with_theme(&ColorfulTheme::default())
                .with_prompt("Select actor type")
                .default(0)
                .items(&actor_types)
                .interact()?;
                
            actor_types[selection].to_string().replace(" ", "-").to_lowercase()
        }
    };
    
    // Set up spinner for project creation
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
            .template("{spinner:.green} {msg}")
            .expect("Invalid spinner template"),
    );
    spinner.set_message("Creating project structure...");
    spinner.enable_steady_tick(Duration::from_millis(80));
    
    // Note: For now we're creating just the basic structure as a proof of concept
    // In the real implementation, we would create all necessary files
    
    // Create a basic structure
    fs::create_dir_all(directory.join("src")).await?;
    
    // Create a basic README
    let readme = format!(
        r#"# {}

A Theater actor implementation in {}.

## Building

```bash
theater dev build
```

## Running

```bash
theater actor start {}.toml
```

## Development

```bash
theater dev watch {}.toml
```
"#,
        name, 
        if language == "rust" { "Rust" } else { "AssemblyScript" },
        name,
        name
    );
    
    fs::write(directory.join("README.md"), readme).await?;
    
    // Create a basic manifest
    let manifest = format!(
        r#"name = "{}"
component_path = "target/wasm.wasm"

[interface]
implements = "ntwk:simple-actor/actor"
requires = []

# Handler configuration for {}
[[handlers]]
type = "{}"
config = {{ port = 8080 }}

[logging]
level = "info"
chain_events = true
"#,
        name,
        actor_type,
        if actor_type == "http-server" { "http-server" } 
        else if actor_type == "message-handler" { "message-server" }
        else if actor_type == "websocket-server" { "websocket-server" }
        else { "runtime" }
    );
    
    fs::write(directory.join(format!("{}.toml", name)), manifest).await?;
    
    // Create basic source file based on language
    let source_file = if language == "rust" {
        directory.join("src/lib.rs")
    } else {
        directory.join("src/index.ts")
    };
    
    let source_content = if language == "rust" {
        r#"// Basic Theater actor implementation

// This is a placeholder. The actual implementation would include proper WIT bindings.
pub fn actor_init() -> bool {
    println!("Actor initialized");
    true
}

pub fn handle_event(event_type: &str, event_data: &str) -> bool {
    println!("Received event: {}", event_type);
    println!("Event data: {}", event_data);
    true
}
"#
    } else {
        r#"// Basic Theater actor implementation

export function actor_init(): boolean {
    console.log("Actor initialized");
    return true;
}

export function handle_event(eventType: string, eventData: string): boolean {
    console.log("Received event: " + eventType);
    console.log("Event data: " + eventData);
    return true;
}
"#
    };
    
    fs::write(source_file, source_content).await?;
    
    // Create a .gitignore file
    let gitignore = if language == "rust" {
        r#"/target
**/*.rs.bk
Cargo.lock
.vscode/
.idea/
"#
    } else {
        r#"node_modules/
dist/
build/
.vscode/
.idea/
"#
    };
    
    fs::write(directory.join(".gitignore"), gitignore).await?;
    
    // Add language-specific files
    if language == "rust" {
        let cargo_toml = format!(
            r#"[package]
name = "{}"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
# Add your dependencies here
"#,
            name.replace("-", "_")
        );
        
        fs::write(directory.join("Cargo.toml"), cargo_toml).await?;
    } else {
        let package_json = format!(
            r#"{{
  "name": "{}",
  "version": "0.1.0",
  "description": "Theater actor implementation",
  "main": "index.js",
  "scripts": {{
    "build": "asc src/index.ts -o target/wasm.wasm"
  }},
  "dependencies": {{
  }}
}}
"#,
            name
        );
        
        fs::write(directory.join("package.json"), package_json).await?;
    }
    
    spinner.finish_with_message(format!("Project created at {}", directory.display()));
    
    println!("\n{} Project scaffolding complete!", style("Success:").green().bold());
    println!("\nNext steps:");
    println!("1. Navigate to the project directory: {}", style(format!("cd {}", directory.display())).yellow());
    println!("2. Build the project: {}", style("theater dev build").yellow());
    println!("3. Run the actor: {}", style(format!("theater actor start {}.toml", name)).yellow());
    
    Ok(())
}

async fn build_wasm_component(
    path_opt: Option<PathBuf>,
    output_opt: Option<PathBuf>,
    release: bool,
) -> Result<()> {
    println!("{}", style("Theater WASM Component Builder").bold().cyan());
    
    // Determine the path to build
    let path = match path_opt {
        Some(p) => p,
        None => {
            let path_str: String = Input::with_theme(&ColorfulTheme::default())
                .with_prompt("Path to source project")
                .default(".".to_string())
                .interact()?;
                
            PathBuf::from(path_str)
        }
    };
    
    // Verify the path exists
    if !path.exists() {
        return Err(anyhow::anyhow!("Source path does not exist: {}", path.display()));
    }
    
    // Try to detect if it's Rust or AssemblyScript
    let is_rust = path.join("Cargo.toml").exists();
    let is_assemblyscript = path.join("package.json").exists() && (
        path.join("assembly").exists() || 
        path.join("src").join("index.ts").exists()
    );
    
    if !is_rust && !is_assemblyscript {
        println!("{} Could not determine project type. Assuming Rust.", 
            style("Warning:").yellow().bold()
        );
    }
    
    let project_type = if is_assemblyscript { "AssemblyScript" } else { "Rust" };
    
    // Determine output path
    let output_dir = match output_opt {
        Some(o) => o,
        None => {
            if is_rust {
                path.join("target").join(if release { "release" } else { "debug" })
            } else {
                path.join("build")
            }
        }
    };
    
    // Set up spinner for build process
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
            .template("{spinner:.green} {msg}")
            .expect("Invalid spinner template"),
    );
    spinner.set_message(format!("Building {} project ({}mode)...", 
        project_type, 
        if release { "release " } else { "debug " }
    ));
    spinner.enable_steady_tick(Duration::from_millis(80));
    
    println!("{} This feature would build a WASM component from your source code.", 
        style("Note:").yellow().bold()
    );
    
    println!("For now, this is just a placeholder. In the future, it would:");
    println!("1. Detect your project type ({})", project_type);
    println!("2. Use the appropriate build system");
    println!("3. Generate WASM components compatible with Theater");
    
    sleep(Duration::from_secs(2)).await;
    
    spinner.finish_with_message("Build simulation complete");
    
    println!("\n{} In a real implementation, your WASM component would be at:", 
        style("Note:").yellow().bold()
    );
    println!("{}", style(output_dir.join("wasm.wasm").display()).green());
    
    Ok(())
}

async fn test_actor_locally(
    manifest: PathBuf,
    script_opt: Option<PathBuf>,
    verbose: bool,
) -> Result<()> {
    println!("{}", style("Theater Actor Tester").bold().cyan());
    
    // Verify the manifest exists
    if !manifest.exists() {
        return Err(anyhow::anyhow!("Manifest file not found: {}", manifest.display()));
    }
    
    println!("{} This feature would test an actor locally without deploying it to a server.", 
        style("Note:").yellow().bold()
    );
    
    println!("\nManifest: {}", style(manifest.display()).green());
    
    if let Some(script) = script_opt {
        println!("Test script: {}", style(script.display()).green());
    } else {
        println!("Using default test behavior");
    }
    
    if verbose {
        println!("Verbose mode enabled");
    }
    
    println!("\n{} Example test output:", style("Sample").underlined());
    println!("[Test] Loading actor from manifest");
    println!("[Test] Initializing actor");
    println!("[Test] Sending test event");
    println!("[Actor] Received event: test");
    println!("[Test] Event handling successful");
    println!("[Test] All tests passed!");
    
    Ok(())
}

async fn watch_and_restart(
    manifest: PathBuf,
    watch_dirs_opt: Option<Vec<PathBuf>>,
    address: String,
) -> Result<()> {
    println!("{}", style("Theater Actor Watcher").bold().cyan());
    
    // Verify the manifest exists
    if !manifest.exists() {
        return Err(anyhow::anyhow!("Manifest file not found: {}", manifest.display()));
    }
    
    // Determine directories to watch
    let watch_dirs = match watch_dirs_opt {
        Some(dirs) => dirs,
        None => {
            // Default to watching the manifest directory and src directory
            let base_dir = manifest.parent().unwrap_or_else(|| Path::new("."));
            vec![
                base_dir.to_path_buf(),
                base_dir.join("src"),
            ]
        }
    };
    
    println!("{} This feature would watch for changes and automatically restart the actor.", 
        style("Note:").yellow().bold()
    );
    
    println!("\nManifest: {}", style(manifest.display()).green());
    println!("Server: {}", style(&address).yellow());
    println!("\nWatching directories:");
    for dir in &watch_dirs {
        println!("- {}", style(dir.display()).green());
    }
    
    println!("\nPress Ctrl+C to stop watching...");
    
    // Simulate watching with a loop
    for i in 1..=5 {
        sleep(Duration::from_secs(3)).await;
        
        // Check for Ctrl+C
        if tokio::select! {
            _ = sleep(Duration::from_millis(10)) => { false }
            _ = tokio::signal::ctrl_c() => { true }
        } {
            println!("\n{} Watching stopped", style("INFO:").blue().bold());
            break;
        }
        
        // Simulate detecting changes
        if i == 3 {
            println!("\n{} Change detected in {}", 
                style("Change:").yellow().bold(),
                style("src/lib.rs").green()
            );
            println!("{} Building project...", style("Action:").blue().bold());
            sleep(Duration::from_secs(1)).await;
            println!("{} Restarting actor...", style("Action:").blue().bold());
            sleep(Duration::from_secs(1)).await;
            println!("{} Actor restarted successfully", style("Success:").green().bold());
        }
    }
    
    Ok(())
}

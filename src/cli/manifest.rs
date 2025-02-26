use crate::config::{
    HandlerConfig, HttpServerHandlerConfig, InterfacesConfig, LoggingConfig, ManifestConfig,
    MessageServerConfig, SupervisorHostConfig, WebSocketServerHandlerConfig,
};
use anyhow::Result;
use clap::{Args, Subcommand};
use console::style;
use dialoguer::{theme::ColorfulTheme, Confirm, Input, MultiSelect, Select};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Args)]
pub struct ManifestArgs {
    #[command(subcommand)]
    pub command: ManifestCommands,
}

#[derive(Subcommand)]
pub enum ManifestCommands {
    /// Create a new actor manifest interactively
    Create {
        /// Path to save the manifest
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Use an existing manifest as a template
        #[arg(short, long)]
        template: Option<PathBuf>,

        /// Name for the actor
        #[arg(short, long)]
        name: Option<String>,

        /// Path to the WASM component
        #[arg(short, long)]
        component: Option<PathBuf>,
    },

    /// Validate a manifest file
    Validate {
        /// Path to the manifest to validate
        manifest: PathBuf,
    },

    /// List available manifest templates
    List {
        /// Show detailed template information
        #[arg(short, long)]
        detailed: bool,
    },
}

pub async fn handle_manifest_command(args: ManifestArgs) -> Result<()> {
    match args.command {
        ManifestCommands::Create {
            output,
            template,
            name,
            component,
        } => create_manifest(output, template, name, component).await,
        ManifestCommands::Validate { manifest } => validate_manifest(manifest).await,
        ManifestCommands::List { detailed } => list_templates(detailed).await,
    }
}

async fn create_manifest(
    output: Option<PathBuf>,
    template: Option<PathBuf>,
    name: Option<String>,
    component: Option<PathBuf>,
) -> Result<()> {
    // Header
    println!("{}", style("Theater Actor Manifest Creator").bold().cyan());
    println!("This tool will help you create a new actor manifest\n");

    // Initialize a new manifest config, optionally from a template
    let mut manifest = if let Some(template_path) = template {
        println!(
            "Using template: {}",
            style(template_path.display()).yellow()
        );
        match ManifestConfig::from_file(&template_path) {
            Ok(config) => config,
            Err(e) => {
                println!(
                    "{} Failed to load template: {}",
                    style("Error:").red().bold(),
                    e
                );
                return Err(e);
            }
        }
    } else {
        // Create a basic default manifest
        ManifestConfig {
            name: String::new(),
            component_path: PathBuf::new(),
            init_state: None,
            interface: InterfacesConfig::default(),
            handlers: Vec::new(),
            logging: LoggingConfig::default(),
            event_server: None,
        }
    };

    // Actor name
    manifest.name = match name {
        Some(n) => n,
        None => {
            let default = if !manifest.name.is_empty() {
                manifest.name.clone()
            } else {
                "my-actor".to_string()
            };

            Input::with_theme(&ColorfulTheme::default())
                .with_prompt("Actor name")
                .default(default)
                .interact()?
        }
    };

    // Component path
    manifest.component_path = match component {
        Some(p) => p,
        None => {
            let default = if !manifest.component_path.as_os_str().is_empty() {
                manifest.component_path.clone()
            } else {
                PathBuf::from(format!("{}.wasm", manifest.name))
            };

            let path_str: String = Input::with_theme(&ColorfulTheme::default())
                .with_prompt("WASM component path")
                .default(default.to_string_lossy().to_string())
                .interact()?;

            PathBuf::from(path_str)
        }
    };

    // Interface
    println!("\n{}", style("Actor Interface").bold().underlined());

    manifest.interface.implements = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Interface implementation")
        .default(if !manifest.interface.implements.is_empty() {
            manifest.interface.implements.clone()
        } else {
            "ntwk:simple-actor/actor".to_string()
        })
        .interact()?;

    // Ask about required interfaces
    let add_required = Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Add required interfaces?")
        .default(false)
        .interact()?;

    if add_required {
        let mut adding = true;
        manifest.interface.requires.clear();

        while adding {
            let interface: String = Input::with_theme(&ColorfulTheme::default())
                .with_prompt("Required interface")
                .interact()?;

            manifest.interface.requires.push(interface);

            adding = Confirm::with_theme(&ColorfulTheme::default())
                .with_prompt("Add another required interface?")
                .default(false)
                .interact()?;
        }
    }

    // Initial state
    let add_initial_state = Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Add initial state file?")
        .default(false)
        .interact()?;

    if add_initial_state {
        // Fixed: Using String as default instead of &str
        let state_path: String = Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Initial state file path")
            .default("initial_state.json".to_string())
            .interact()?;

        manifest.init_state = Some(PathBuf::from(state_path));
    } else {
        manifest.init_state = None;
    }

    // Handlers
    println!("\n{}", style("Actor Handlers").bold().underlined());
    println!("Select the handlers you want to enable:\n");

    // Define available handlers
    let available_handlers = vec![
        "HTTP Server",
        "Message Server",
        "WebSocket Server",
        "Supervisor",
    ];

    let selections = MultiSelect::with_theme(&ColorfulTheme::default())
        .with_prompt("Select handlers")
        .items(&available_handlers)
        .defaults(&vec![false; available_handlers.len()])
        .interact()?;

    // Clear existing handlers and add new ones based on selection
    manifest.handlers.clear();

    for &idx in selections.iter() {
        match available_handlers[idx] {
            "HTTP Server" => {
                let port: u16 = Input::with_theme(&ColorfulTheme::default())
                    .with_prompt("HTTP server port")
                    .default(8080)
                    .interact()?;

                manifest
                    .handlers
                    .push(HandlerConfig::HttpServer(HttpServerHandlerConfig { port }));
            }
            "Message Server" => {
                let port: u16 = Input::with_theme(&ColorfulTheme::default())
                    .with_prompt("Message server port")
                    .default(9090)
                    .interact()?;

                manifest
                    .handlers
                    .push(HandlerConfig::MessageServer(MessageServerConfig { port }));
            }
            "WebSocket Server" => {
                let port: u16 = Input::with_theme(&ColorfulTheme::default())
                    .with_prompt("WebSocket server port")
                    .default(8090)
                    .interact()?;

                manifest.handlers.push(HandlerConfig::WebSocketServer(
                    WebSocketServerHandlerConfig { port },
                ));
            }
            "Supervisor" => {
                manifest
                    .handlers
                    .push(HandlerConfig::Supervisor(SupervisorHostConfig {}));
            }
            _ => {}
        }
    }

    // Logging
    println!("\n{}", style("Logging Configuration").bold().underlined());

    let levels = vec!["error", "warn", "info", "debug", "trace"];
    let default_idx = levels
        .iter()
        .position(|&l| l == manifest.logging.level.as_str())
        .unwrap_or(2); // Default to "info"

    let level_idx = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Logging level")
        .default(default_idx)
        .items(&levels)
        .interact()?;

    manifest.logging.level = levels[level_idx].to_string();

    let log_chain_events = Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Log chain events?")
        .default(manifest.logging.chain_events)
        .interact()?;

    manifest.logging.chain_events = log_chain_events;

    // Convert the manifest to TOML
    let toml = toml::to_string_pretty(&manifest)?;

    // Determine output path
    let output_path = match output {
        Some(path) => path,
        None => {
            let default = format!("{}.toml", manifest.name);
            let path_str: String = Input::with_theme(&ColorfulTheme::default())
                .with_prompt("Save manifest as")
                .default(default)
                .interact()?;

            PathBuf::from(path_str)
        }
    };

    // Check if file exists
    if output_path.exists() {
        let overwrite = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(&format!(
                "File {} already exists. Overwrite?",
                output_path.display()
            ))
            .default(false)
            .interact()?;

        if !overwrite {
            println!(
                "{} Manifest creation canceled",
                style("Canceled:").yellow().bold()
            );
            return Ok(());
        }
    }

    // Create parent directories if needed
    if let Some(parent) = output_path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }

    // Write the manifest
    fs::write(&output_path, toml)?;

    println!(
        "\n{} Manifest created: {}",
        style("Success:").green().bold(),
        style(output_path.display()).green()
    );

    // Offer to view the content
    let view_content = Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("View the manifest content?")
        .default(true)
        .interact()?;

    if view_content {
        println!("\n{}", style("Manifest Content:").bold().underlined());
        println!("{}", fs::read_to_string(&output_path)?);
    }

    Ok(())
}

async fn validate_manifest(manifest_path: PathBuf) -> Result<()> {
    println!("{}", style("Validating manifest...").bold().cyan());

    if !manifest_path.exists() {
        println!(
            "{} File not found: {}",
            style("Error:").red().bold(),
            manifest_path.display()
        );
        return Err(anyhow::anyhow!("Manifest file not found"));
    }

    match ManifestConfig::from_file(&manifest_path) {
        Ok(config) => {
            // Basic validation
            let mut issues = Vec::new();

            // Check component path
            if !Path::new(&config.component_path).exists() {
                issues.push(format!(
                    "Component file not found: {}",
                    config.component_path.display()
                ));
            }

            // Check initial state if specified
            if let Some(path) = config.init_state {
                if !Path::new(&path).exists() {
                    issues.push(format!("Initial state file not found: {}", path.display()));
                }
            }

            // Check interface
            if config.interface.implements.is_empty() {
                issues.push("Missing actor interface implementation".to_string());
            }

            // Check for port conflicts
            let mut ports = Vec::new();
            for handler in &config.handlers {
                match handler {
                    HandlerConfig::HttpServer(h) => ports.push(("HTTP Server", h.port)),
                    HandlerConfig::MessageServer(h) => ports.push(("Message Server", h.port)),
                    HandlerConfig::WebSocketServer(h) => ports.push(("WebSocket Server", h.port)),
                    _ => {}
                }
            }

            // Check for duplicate ports
            let mut duplicate_ports = Vec::new();
            for i in 0..ports.len() {
                for j in i + 1..ports.len() {
                    if ports[i].1 == ports[j].1 {
                        duplicate_ports.push(format!(
                            "Port conflict: {} and {} both use port {}",
                            ports[i].0, ports[j].0, ports[i].1
                        ));
                    }
                }
            }
            issues.extend(duplicate_ports);

            // Report validation results
            if issues.is_empty() {
                println!("{} Manifest is valid", style("Success:").green().bold());

                // Show summary
                println!("\n{}", style("Manifest Summary:").bold().underlined());
                println!("Actor Name: {}", style(&config.name).green());
                println!("Component: {}", config.component_path.display());
                println!("Interface: {}", config.interface.implements);

                if !config.interface.requires.is_empty() {
                    println!("Required Interfaces:");
                    for interface in &config.interface.requires {
                        println!("  - {}", interface);
                    }
                }

                if !config.handlers.is_empty() {
                    println!("\nHandlers:");
                    for handler in &config.handlers {
                        match handler {
                            HandlerConfig::HttpServer(h) => {
                                println!("  - HTTP Server (port {})", h.port)
                            }
                            HandlerConfig::MessageServer(h) => {
                                println!("  - Message Server (port {})", h.port)
                            }
                            HandlerConfig::WebSocketServer(h) => {
                                println!("  - WebSocket Server (port {})", h.port)
                            }
                            HandlerConfig::Supervisor(_) => println!("  - Supervisor"),
                            HandlerConfig::FileSystem(h) => {
                                println!("  - File System (path: {})", h.path.display())
                            }
                            HandlerConfig::HttpClient(_) => println!("  - HTTP Client"),
                            HandlerConfig::Runtime(_) => println!("  - Runtime"),
                        }
                    }
                }
            } else {
                println!(
                    "{} Manifest validation failed",
                    style("Error:").red().bold()
                );
                println!("\n{}", style("Issues found:").bold().underlined());
                for (i, issue) in issues.iter().enumerate() {
                    println!("{}. {}", i + 1, style(issue).red());
                }
            }

            Ok(())
        }
        Err(e) => {
            println!(
                "{} Failed to parse manifest: {}",
                style("Error:").red().bold(),
                e
            );
            Err(e)
        }
    }
}

async fn list_templates(detailed: bool) -> Result<()> {
    println!("{}", style("Available Manifest Templates").bold().cyan());

    // Default location for templates
    let template_dirs = vec![PathBuf::from("examples"), PathBuf::from("./templates")];

    let mut templates = Vec::new();

    for dir in template_dirs {
        if dir.exists() && dir.is_dir() {
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();

                if path.is_file() && path.extension().map_or(false, |ext| ext == "toml") {
                    // Try to parse as manifest to verify
                    match ManifestConfig::from_file(&path) {
                        Ok(config) => {
                            templates.push((path, config));
                        }
                        Err(_) => {
                            // Not a valid manifest template, skip
                        }
                    }
                }
            }
        }
    }

    if templates.is_empty() {
        println!("No templates found in standard directories.");
        println!("You can create templates by saving manifest files in:");
        println!("  - ./examples/*.toml");
        println!("  - ./templates/*.toml");
        return Ok(());
    }

    println!("Found {} templates:", templates.len());

    for (i, (path, config)) in templates.iter().enumerate() {
        println!(
            "\n{}. {} ({})",
            i + 1,
            style(&config.name).green(),
            path.display()
        );

        if detailed {
            println!("   Interface: {}", config.interface.implements);

            let mut handler_types = Vec::new();
            for handler in &config.handlers {
                match handler {
                    HandlerConfig::HttpServer(_) => handler_types.push("HTTP Server"),
                    HandlerConfig::MessageServer(_) => handler_types.push("Message Server"),
                    HandlerConfig::WebSocketServer(_) => handler_types.push("WebSocket Server"),
                    HandlerConfig::Supervisor(_) => handler_types.push("Supervisor"),
                    HandlerConfig::FileSystem(_) => handler_types.push("File System"),
                    HandlerConfig::HttpClient(_) => handler_types.push("HTTP Client"),
                    HandlerConfig::Runtime(_) => handler_types.push("Runtime"),
                }
            }

            if !handler_types.is_empty() {
                println!("   Handlers: {}", handler_types.join(", "));
            }
        }
    }

    println!("\nUse 'theater manifest create --template <path>' to create a new manifest based on a template.");

    Ok(())
}

use chrono::DateTime;
use serde_json::Value;
use theater::ChainEvent;

use crate::error::CliResult;
use crate::output::{OutputFormat, OutputManager};
use crate::utils::event_display::display_single_event;

/// Actor list formatter
#[derive(Debug, serde::Serialize)]
pub struct ActorList {
    pub actors: Vec<(String, String)>,
}

impl OutputFormat for ActorList {
    fn format_compact(&self, _output: &OutputManager) -> CliResult<()> {
        // Script-friendly output: just actor_id name, one per line
        for (id, name) in &self.actors {
            println!("{} {}", id, name);
        }
        Ok(())
    }

    fn format_pretty(&self, _output: &OutputManager) -> CliResult<()> {
        // Use same script-friendly output for consistency
        for (id, name) in &self.actors {
            println!("{} {}", id, name);
        }
        Ok(())
    }

    fn format_table(&self, _output: &OutputManager) -> CliResult<()> {
        // Use same script-friendly output for consistency
        for (id, name) in &self.actors {
            println!("{} {}", id, name);
        }
        Ok(())
    }

    fn format_detailed(&self, _output: &OutputManager) -> CliResult<()> {
        // Use same script-friendly output for consistency
        for (id, name) in &self.actors {
            println!("{} {}", id, name);
        }
        Ok(())
    }
}

/// Actor events formatter
#[derive(Debug, serde::Serialize)]
pub struct ActorEvents {
    pub actor_id: String,
    pub events: Vec<ChainEvent>,
}

impl OutputFormat for ActorEvents {
    fn format_compact(&self, output: &OutputManager) -> CliResult<()> {
        if self.events.is_empty() {
            output.info(&format!("No events found for actor {}", self.actor_id))?;
            return Ok(());
        }

        println!(
            "{:<12} {:<12} {:<25} {}",
            "HASH", "PARENT", "EVENT TYPE", "DESCRIPTION"
        );
        println!("{}", "─".repeat(100));

        for event in &self.events {
            display_single_event(event, "compact")?;
        }
        Ok(())
    }

    fn format_pretty(&self, output: &OutputManager) -> CliResult<()> {
        if self.events.is_empty() {
            output.info(&format!("No events found for actor {}", self.actor_id))?;
            return Ok(());
        }

        println!(
            "{}",
            output
                .theme()
                .highlight()
                .apply_to(&format!("Events for Actor: {}", self.actor_id))
        );
        println!("{}", "─".repeat(80));

        for event in self.events.iter() {
            display_single_event(event, "pretty")?;
        }

        Ok(())
    }

    fn format_table(&self, output: &OutputManager) -> CliResult<()> {
        if self.events.is_empty() {
            output.info(&format!("No events found for actor {}", self.actor_id))?;
            return Ok(());
        }

        let headers = vec!["Timestamp", "Type", "Description"];
        let rows: Vec<Vec<String>> = self
            .events
            .iter()
            .map(|event| {
                vec![
                    format_timestamp(event.timestamp),
                    event.event_type.clone(),
                    truncate_string(event.description.as_deref().unwrap_or("No description"), 50),
                ]
            })
            .collect();

        output.table(&headers, &rows)?;
        Ok(())
    }

    fn format_detailed(&self, output: &OutputManager) -> CliResult<()> {
        if self.events.is_empty() {
            output.info(&format!("No events found for actor {}", self.actor_id))?;
            return Ok(());
        }

        println!(
            "{}",
            output.theme().highlight().apply_to("Detailed Actor Events")
        );
        println!("{}", "─".repeat(80));

        for event in self.events.iter() {
            display_single_event(event, "detailed")?;
        }
        Ok(())
    }
}

/// Actor state formatter
#[derive(Debug, serde::Serialize)]
pub struct ActorState {
    pub actor_id: String,
    pub state: Value,
}

impl OutputFormat for ActorState {
    fn format_compact(&self, output: &OutputManager) -> CliResult<()> {
        println!(
            "State for actor {}: {}",
            output.theme().accent().apply_to(&self.actor_id),
            self.state
        );
        Ok(())
    }

    fn format_pretty(&self, output: &OutputManager) -> CliResult<()> {
        println!(
            "{}",
            output
                .theme()
                .highlight()
                .apply_to(&format!("State for Actor: {}", self.actor_id))
        );
        println!("{}", "─".repeat(40));
        println!(
            "{}",
            serde_json::to_string_pretty(&self.state)
                .unwrap_or_else(|_| "Invalid JSON".to_string())
        );
        Ok(())
    }

    fn format_table(&self, output: &OutputManager) -> CliResult<()> {
        // For state, table format doesn't make much sense, so fall back to pretty
        self.format_pretty(output)
    }

    fn format_detailed(&self, output: &OutputManager) -> CliResult<()> {
        println!(
            "{}",
            output.theme().highlight().apply_to("Detailed Actor State")
        );
        println!("{}", "─".repeat(40));
        println!(
            "{}",
            serde_json::to_string_pretty(&self.state)
                .unwrap_or_else(|_| "Invalid JSON".to_string())
        );
        Ok(())
    }
}

/// Build result formatter
#[derive(Debug, serde::Serialize)]
pub struct BuildResult {
    pub success: bool,
    pub project_dir: std::path::PathBuf,
    pub wasm_path: Option<std::path::PathBuf>,
    pub manifest_exists: bool,
    pub manifest_path: Option<std::path::PathBuf>,
    pub build_type: String,
    pub package_name: String,
    pub stdout: String,
    pub stderr: String,
}

impl OutputFormat for BuildResult {
    fn format_compact(&self, output: &OutputManager) -> CliResult<()> {
        if self.success {
            output.success("Build completed successfully")?;
            if let Some(wasm_path) = &self.wasm_path {
                println!(
                    "  Component: {}",
                    output.theme().accent().apply_to(wasm_path.display())
                );
            }
        } else {
            output.error("Build failed")?;
            if !self.stderr.is_empty() {
                println!("{}", output.theme().muted().apply_to(&self.stderr));
            }
        }
        Ok(())
    }

    fn format_pretty(&self, output: &OutputManager) -> CliResult<()> {
        if self.success {
            println!(
                "{} {}",
                output.theme().success_icon(),
                output.theme().highlight().apply_to("Build Successful")
            );
            println!();
            println!(
                "Package: {}",
                output.theme().accent().apply_to(&self.package_name)
            );
            println!(
                "Build Type: {}",
                output.theme().muted().apply_to(&self.build_type)
            );

            if let Some(wasm_path) = &self.wasm_path {
                println!(
                    "Component: {}",
                    output.theme().accent().apply_to(wasm_path.display())
                );
            }

            if self.manifest_exists {
                if let Some(manifest_path) = &self.manifest_path {
                    println!("\nTo deploy your actor:");
                    println!(
                        "  theater start {}",
                        output.theme().muted().apply_to(manifest_path.display())
                    );
                }
            } else {
                println!(
                    "\n{} No manifest.toml found.",
                    output.theme().warning_icon()
                );
                if let Some(wasm_path) = &self.wasm_path {
                    println!(
                        "Create one to deploy: theater create-manifest --component-path {}",
                        output.theme().muted().apply_to(wasm_path.display())
                    );
                }
            }

            if !self.stdout.is_empty() {
                println!("\nBuild Output:");
                println!("{}", output.theme().muted().apply_to(&self.stdout));
            }
        } else {
            println!(
                "{} {}",
                output.theme().error_icon(),
                output.theme().error().apply_to("Build Failed")
            );

            if !self.stderr.is_empty() {
                println!("\nError Output:");
                println!("{}", self.stderr);
            }
            if !self.stdout.is_empty() {
                println!("\nBuild Output:");
                println!("{}", self.stdout);
            }
        }
        Ok(())
    }

    fn format_table(&self, output: &OutputManager) -> CliResult<()> {
        let headers = vec!["Property", "Value"];
        let mut rows = vec![vec![
            "Status".to_string(),
            if self.success {
                "Success".to_string()
            } else {
                "Failed".to_string()
            },
        ]];

        rows.push(vec!["Package".to_string(), self.package_name.clone()]);
        rows.push(vec!["Build Type".to_string(), self.build_type.clone()]);
        rows.push(vec![
            "Project Dir".to_string(),
            self.project_dir.display().to_string(),
        ]);

        if let Some(wasm_path) = &self.wasm_path {
            rows.push(vec![
                "Component".to_string(),
                wasm_path.display().to_string(),
            ]);
        }

        rows.push(vec![
            "Manifest Exists".to_string(),
            self.manifest_exists.to_string(),
        ]);

        if let Some(manifest_path) = &self.manifest_path {
            rows.push(vec![
                "Manifest Path".to_string(),
                manifest_path.display().to_string(),
            ]);
        }

        output.table(&headers, &rows)?;
        Ok(())
    }

    fn format_detailed(&self, _output: &OutputManager) -> CliResult<()> {
        todo!()
    }
}

/// Actor action result formatter (for start, stop, restart actions)
#[derive(Debug, serde::Serialize)]
pub struct ActorAction {
    pub action: String,
    pub actor_id: String,
    pub success: bool,
    pub message: Option<String>,
}

impl OutputFormat for ActorAction {
    fn format_compact(&self, output: &OutputManager) -> CliResult<()> {
        if self.success {
            println!(
                "{} {} actor: {}",
                output.theme().success_icon(),
                self.action
                    .chars()
                    .next()
                    .unwrap()
                    .to_uppercase()
                    .collect::<String>()
                    + &self.action[1..],
                output.theme().accent().apply_to(&self.actor_id)
            );
        } else {
            println!(
                "{} Failed to {} actor: {}",
                output.theme().error_icon(),
                self.action,
                output.theme().accent().apply_to(&self.actor_id)
            );
            if let Some(msg) = &self.message {
                println!("{}", output.theme().muted().apply_to(msg));
            }
        }
        Ok(())
    }

    fn format_pretty(&self, output: &OutputManager) -> CliResult<()> {
        if self.success {
            println!(
                "{} {}",
                output.theme().success_icon(),
                output.theme().highlight().apply_to(&format!(
                    "Actor {} Successfully",
                    self.action
                        .chars()
                        .next()
                        .unwrap()
                        .to_uppercase()
                        .collect::<String>()
                        + &self.action[1..]
                ))
            );
            println!(
                "Actor ID: {}",
                output.theme().accent().apply_to(&self.actor_id)
            );
        } else {
            println!(
                "{} {}",
                output.theme().error_icon(),
                output
                    .theme()
                    .error()
                    .apply_to(&format!("Failed to {} Actor", self.action))
            );
            println!(
                "Actor ID: {}",
                output.theme().accent().apply_to(&self.actor_id)
            );
            if let Some(msg) = &self.message {
                println!("Error: {}", msg);
            }
        }
        Ok(())
    }

    fn format_table(&self, output: &OutputManager) -> CliResult<()> {
        let headers = vec!["Property", "Value"];
        let mut rows = vec![
            vec!["Action".to_string(), self.action.clone()],
            vec!["Actor ID".to_string(), self.actor_id.clone()],
            vec![
                "Status".to_string(),
                if self.success {
                    "Success".to_string()
                } else {
                    "Failed".to_string()
                },
            ],
        ];

        if let Some(msg) = &self.message {
            rows.push(vec!["Message".to_string(), msg.clone()]);
        }

        output.table(&headers, &rows)?;
        Ok(())
    }

    fn format_detailed(&self, _output: &OutputManager) -> CliResult<()> {
        todo!()
    }
}

/// Component update result formatter
#[derive(Debug, serde::Serialize)]
pub struct ComponentUpdate {
    pub actor_id: String,
    pub component: String,
    pub success: bool,
    pub message: Option<String>,
}

impl OutputFormat for ComponentUpdate {
    fn format_compact(&self, output: &OutputManager) -> CliResult<()> {
        if self.success {
            println!(
                "{} Updated actor: {} with component: {}",
                output.theme().success_icon(),
                output.theme().accent().apply_to(&self.actor_id),
                output.theme().accent().apply_to(&self.component)
            );
        } else {
            println!(
                "{} Failed to update actor: {}",
                output.theme().error_icon(),
                output.theme().accent().apply_to(&self.actor_id)
            );
            if let Some(msg) = &self.message {
                println!("{}", output.theme().muted().apply_to(msg));
            }
        }
        Ok(())
    }

    fn format_pretty(&self, output: &OutputManager) -> CliResult<()> {
        if self.success {
            println!(
                "{} {}",
                output.theme().success_icon(),
                output
                    .theme()
                    .highlight()
                    .apply_to("Component Updated Successfully")
            );
            println!(
                "Actor ID: {}",
                output.theme().accent().apply_to(&self.actor_id)
            );
            println!(
                "New Component: {}",
                output.theme().accent().apply_to(&self.component)
            );
        } else {
            println!(
                "{} {}",
                output.theme().error_icon(),
                output
                    .theme()
                    .error()
                    .apply_to("Failed to Update Component")
            );
            println!(
                "Actor ID: {}",
                output.theme().accent().apply_to(&self.actor_id)
            );
            println!(
                "Component: {}",
                output.theme().accent().apply_to(&self.component)
            );
            if let Some(msg) = &self.message {
                println!("Error: {}", msg);
            }
        }
        Ok(())
    }

    fn format_table(&self, output: &OutputManager) -> CliResult<()> {
        let headers = vec!["Property", "Value"];
        let mut rows = vec![
            vec!["Actor ID".to_string(), self.actor_id.clone()],
            vec!["Component".to_string(), self.component.clone()],
            vec![
                "Status".to_string(),
                if self.success {
                    "Success".to_string()
                } else {
                    "Failed".to_string()
                },
            ],
        ];

        if let Some(msg) = &self.message {
            rows.push(vec!["Message".to_string(), msg.clone()]);
        }

        output.table(&headers, &rows)?;
        Ok(())
    }
    fn format_detailed(&self, _output: &OutputManager) -> CliResult<()> {
        todo!()
    }
}

/// Message sent result formatter
#[derive(Debug, serde::Serialize)]
pub struct MessageSent {
    pub actor_id: String,
    pub message: String,
    pub success: bool,
}

impl OutputFormat for MessageSent {
    fn format_compact(&self, output: &OutputManager) -> CliResult<()> {
        if self.success {
            println!(
                "{} Message sent to actor: {}",
                output.theme().success_icon(),
                output.theme().accent().apply_to(&self.actor_id)
            );
        } else {
            println!(
                "{} Failed to send message to actor: {}",
                output.theme().error_icon(),
                output.theme().accent().apply_to(&self.actor_id)
            );
        }
        Ok(())
    }

    fn format_pretty(&self, output: &OutputManager) -> CliResult<()> {
        if self.success {
            println!(
                "{} {}",
                output.theme().success_icon(),
                output
                    .theme()
                    .highlight()
                    .apply_to("Message Sent Successfully")
            );
            println!(
                "Actor ID: {}",
                output.theme().accent().apply_to(&self.actor_id)
            );
            println!(
                "Message: {}",
                output
                    .theme()
                    .muted()
                    .apply_to(&truncate_string(&self.message, 100))
            );
        } else {
            println!(
                "{} {}",
                output.theme().error_icon(),
                output.theme().error().apply_to("Failed to Send Message")
            );
            println!(
                "Actor ID: {}",
                output.theme().accent().apply_to(&self.actor_id)
            );
        }
        Ok(())
    }

    fn format_table(&self, output: &OutputManager) -> CliResult<()> {
        let headers = vec!["Property", "Value"];
        let rows = vec![
            vec!["Actor ID".to_string(), self.actor_id.clone()],
            vec!["Message".to_string(), truncate_string(&self.message, 100)],
            vec![
                "Status".to_string(),
                if self.success {
                    "Sent".to_string()
                } else {
                    "Failed".to_string()
                },
            ],
        ];

        output.table(&headers, &rows)?;
        Ok(())
    }
    fn format_detailed(&self, _output: &OutputManager) -> CliResult<()> {
        todo!()
    }
}

/// Message response formatter (for request/response)
#[derive(Debug, serde::Serialize)]
pub struct MessageResponse {
    pub actor_id: String,
    pub request: String,
    pub response: String,
}

impl OutputFormat for MessageResponse {
    fn format_compact(&self, output: &OutputManager) -> CliResult<()> {
        println!(
            "{} Response from actor: {}",
            output.theme().success_icon(),
            output.theme().accent().apply_to(&self.actor_id)
        );
        println!("{}", self.response);
        Ok(())
    }

    fn format_pretty(&self, output: &OutputManager) -> CliResult<()> {
        println!(
            "{} {}",
            output.theme().success_icon(),
            output.theme().highlight().apply_to("Response Received")
        );
        println!(
            "Actor ID: {}",
            output.theme().accent().apply_to(&self.actor_id)
        );
        println!(
            "Request: {}",
            output
                .theme()
                .muted()
                .apply_to(&truncate_string(&self.request, 100))
        );
        println!("Response:");
        println!("{}", self.response);
        Ok(())
    }

    fn format_table(&self, output: &OutputManager) -> CliResult<()> {
        let headers = vec!["Property", "Value"];
        let rows = vec![
            vec!["Actor ID".to_string(), self.actor_id.clone()],
            vec!["Request".to_string(), truncate_string(&self.request, 100)],
            vec!["Response".to_string(), truncate_string(&self.response, 100)],
        ];

        output.table(&headers, &rows)?;
        Ok(())
    }
    fn format_detailed(&self, _output: &OutputManager) -> CliResult<()> {
        todo!()
    }
}

/// Stored actor list formatter
#[derive(Debug, serde::Serialize)]
pub struct StoredActorList {
    pub actor_ids: Vec<String>,
    pub chains_dir: String,
    pub directory_exists: bool,
}

impl OutputFormat for StoredActorList {
    fn format_compact(&self, output: &OutputManager) -> CliResult<()> {
        if !self.directory_exists {
            output.info(&format!(
                "No stored actors found. Chains directory does not exist: {}",
                self.chains_dir
            ))?;
        } else if self.actor_ids.is_empty() {
            output.info("No stored actors found")?;
        } else {
            output.info(&format!("Stored actors: {}", self.actor_ids.len()))?;
            for actor_id in &self.actor_ids {
                println!("  {}", output.theme().accent().apply_to(actor_id));
            }
        }
        Ok(())
    }

    fn format_pretty(&self, output: &OutputManager) -> CliResult<()> {
        if !self.directory_exists {
            println!(
                "{} {}",
                output.theme().info_icon(),
                output
                    .theme()
                    .highlight()
                    .apply_to("No Stored Actors Found")
            );
            println!(
                "Chains directory does not exist: {}",
                output.theme().muted().apply_to(&self.chains_dir)
            );
        } else {
            println!(
                "{} {}",
                output.theme().info_icon(),
                output
                    .theme()
                    .highlight()
                    .apply_to(&format!("Stored Actors ({})", self.actor_ids.len()))
            );
            println!(
                "Directory: {}",
                output.theme().muted().apply_to(&self.chains_dir)
            );
            println!("{}", "─".repeat(40));

            if self.actor_ids.is_empty() {
                println!("No stored actors found.");
            } else {
                for (i, actor_id) in self.actor_ids.iter().enumerate() {
                    println!("{}. {}", i + 1, output.theme().accent().apply_to(actor_id));
                }
            }
        }
        Ok(())
    }

    fn format_table(&self, output: &OutputManager) -> CliResult<()> {
        if !self.directory_exists || self.actor_ids.is_empty() {
            self.format_pretty(output)?;
            return Ok(());
        }

        let headers = vec!["#", "Actor ID"];
        let rows: Vec<Vec<String>> = self
            .actor_ids
            .iter()
            .enumerate()
            .map(|(i, id)| vec![(i + 1).to_string(), id.clone()])
            .collect();

        output.table(&headers, &rows)?;
        Ok(())
    }
    fn format_detailed(&self, _output: &OutputManager) -> CliResult<()> {
        todo!()
    }
}

/// Actor logs formatter
#[derive(Debug, serde::Serialize)]
pub struct ActorLogs {
    pub actor_id: String,
    pub events: Vec<ChainEvent>,
    pub follow_mode: bool,
    pub lines_limit: usize,
}

impl OutputFormat for ActorLogs {
    fn format_compact(&self, output: &OutputManager) -> CliResult<()> {
        println!(
            "{} Logs for actor: {}",
            output.theme().info_icon(),
            output.theme().accent().apply_to(&self.actor_id)
        );

        if self.events.is_empty() {
            println!("  No logs found.");
        } else {
            for event in &self.events {
                if let Ok(json_data) = serde_json::from_slice::<serde_json::Value>(&event.data) {
                    if let Some(message) = json_data.get("message").and_then(|m| m.as_str()) {
                        println!("[{}] {}", format_timestamp(event.timestamp), message);
                    }
                }
            }
        }
        Ok(())
    }

    fn format_pretty(&self, output: &OutputManager) -> CliResult<()> {
        println!(
            "{} {}",
            output.theme().info_icon(),
            output
                .theme()
                .highlight()
                .apply_to(&format!("Logs for Actor: {}", self.actor_id))
        );

        if self.lines_limit > 0 {
            println!("Showing last {} lines", self.lines_limit);
        }

        println!("{}", "─".repeat(80));

        if self.events.is_empty() {
            println!("No logs found.");
        } else {
            for event in &self.events {
                let timestamp = format_timestamp(event.timestamp);
                if let Ok(json_data) = serde_json::from_slice::<serde_json::Value>(&event.data) {
                    if let Some(message) = json_data.get("message").and_then(|m| m.as_str()) {
                        println!(
                            "{} {}",
                            output.theme().muted().apply_to(&timestamp),
                            message
                        );
                    }
                }
            }
        }

        if self.follow_mode {
            println!();
            output.info("Following logs in real-time. Press Ctrl+C to exit.")?;
        }

        Ok(())
    }

    fn format_table(&self, output: &OutputManager) -> CliResult<()> {
        if self.events.is_empty() {
            output.info(&format!("No logs found for actor: {}", self.actor_id))?;
            return Ok(());
        }

        let headers = vec!["Timestamp", "Message"];
        let rows: Vec<Vec<String>> = self
            .events
            .iter()
            .filter_map(|event| {
                serde_json::from_slice::<serde_json::Value>(&event.data)
                    .ok()
                    .and_then(|json_data| {
                        json_data
                            .get("message")
                            .and_then(|m| m.as_str())
                            .map(|message| {
                                vec![
                                    format_timestamp(event.timestamp),
                                    truncate_string(message, 80),
                                ]
                            })
                    })
            })
            .collect();

        output.table(&headers, &rows)?;
        Ok(())
    }
    fn format_detailed(&self, _output: &OutputManager) -> CliResult<()> {
        todo!()
    }
}

/// Server information formatter
#[derive(Debug, serde::Serialize)]
pub struct ServerInfo {
    pub info: Value,
}

impl OutputFormat for ServerInfo {
    fn format_compact(&self, output: &OutputManager) -> CliResult<()> {
        if let Some(version) = self.info.get("version") {
            println!(
                "Theater Server {}",
                output.theme().accent().apply_to(version)
            );
        }
        if let Some(uptime) = self.info.get("uptime") {
            println!("Uptime: {}", output.theme().muted().apply_to(uptime));
        }
        Ok(())
    }

    fn format_pretty(&self, output: &OutputManager) -> CliResult<()> {
        println!(
            "{}",
            output.theme().highlight().apply_to("Server Information")
        );
        println!("{}", "─".repeat(40));
        println!(
            "{}",
            serde_json::to_string_pretty(&self.info).unwrap_or_else(|_| "Invalid JSON".to_string())
        );
        Ok(())
    }

    fn format_table(&self, output: &OutputManager) -> CliResult<()> {
        // Convert JSON object to key-value table
        if let Value::Object(map) = &self.info {
            let headers = vec!["Property", "Value"];
            let rows: Vec<Vec<String>> = map
                .iter()
                .map(|(key, value)| {
                    vec![
                        key.clone(),
                        match value {
                            Value::String(s) => s.clone(),
                            _ => value.to_string(),
                        },
                    ]
                })
                .collect();
            output.table(&headers, &rows)?;
        } else {
            self.format_pretty(output)?;
        }
        Ok(())
    }
    fn format_detailed(&self, _output: &OutputManager) -> CliResult<()> {
        todo!()
    }
}

/// Actor inspection formatter
#[derive(Debug, serde::Serialize)]
pub struct ActorInspection {
    pub id: theater::id::TheaterId,
    pub status: String,
    pub state: Option<serde_json::Value>,
    pub events: Vec<theater::ChainEvent>,
    pub metrics: Option<serde_json::Value>,
    pub detailed: bool,
}

impl OutputFormat for ActorInspection {
    fn format_compact(&self, output: &OutputManager) -> CliResult<()> {
        println!(
            "{} - {}",
            output.theme().accent().apply_to(&self.id.to_string()),
            output.theme().muted().apply_to(&self.status)
        );
        if let Some(ref state) = self.state {
            let state_str =
                serde_json::to_string(state).unwrap_or_else(|_| "Invalid JSON".to_string());
            if state_str.len() > 100 {
                println!("State: {} bytes", state_str.len());
            } else {
                println!("State: {}", truncate_string(&state_str, 100));
            }
        } else {
            println!("State: null");
        }
        println!("Events: {}", self.events.len());
        Ok(())
    }

    fn format_pretty(&self, output: &OutputManager) -> CliResult<()> {
        use std::time::Duration;

        println!(
            "{}",
            output.theme().highlight().apply_to("ACTOR INFORMATION")
        );
        println!("{}", "─".repeat(50));
        println!(
            "ID: {}",
            output.theme().accent().apply_to(&self.id.to_string())
        );
        println!("Status: {}", output.theme().muted().apply_to(&self.status));

        // Calculate uptime if we have events
        if let Some(first_event) = self.events.first() {
            let now = chrono::Utc::now().timestamp() as u64;
            let uptime = Duration::from_secs(now.saturating_sub(first_event.timestamp));
            println!(
                "Uptime: {}",
                crate::utils::formatting::format_duration(uptime)
            );
        }

        println!();
        println!("{}", output.theme().highlight().apply_to("STATE"));
        println!("{}", "─".repeat(50));
        match &self.state {
            Some(state_json) => {
                let state_str = serde_json::to_string_pretty(state_json)
                    .unwrap_or_else(|_| "Invalid JSON".to_string());
                if state_str.len() < 1000 || self.detailed {
                    println!("{}", state_str);
                } else {
                    println!("{} bytes of JSON data", state_str.len());
                    println!("(Use --detailed to see full state)");
                }
            }
            None => println!("State is null"),
        }

        println!();
        println!("{}", output.theme().highlight().apply_to("EVENTS"));
        println!("{}", "─".repeat(50));
        println!("Total events: {}", self.events.len());

        if !self.events.is_empty() {
            println!();
            println!("Latest events:");
            let start_idx = if self.events.len() > 5 && !self.detailed {
                self.events.len() - 5
            } else {
                0
            };

            for (i, event) in self.events.iter().enumerate().skip(start_idx) {
                println!(
                    "{}. {}",
                    i + 1,
                    crate::utils::formatting::format_event_summary(event)
                );
            }

            if self.events.len() > 5 && !self.detailed {
                println!();
                println!("(Showing only the last 5 events. Use --detailed to see all.)");
            }
        }

        // Metrics information if available
        if let Some(ref metrics) = self.metrics {
            println!();
            println!("{}", output.theme().highlight().apply_to("METRICS"));
            println!("{}", "─".repeat(50));
            println!(
                "{}",
                serde_json::to_string_pretty(metrics)
                    .unwrap_or_else(|_| "Invalid JSON".to_string())
            );
        }

        Ok(())
    }

    fn format_table(&self, output: &OutputManager) -> CliResult<()> {
        let headers = vec!["Property", "Value"];
        let mut rows = vec![
            vec!["ID".to_string(), self.id.to_string()],
            vec!["Status".to_string(), self.status.clone()],
        ];

        if let Some(ref state) = self.state {
            let state_str =
                serde_json::to_string(state).unwrap_or_else(|_| "Invalid JSON".to_string());
            rows.push(vec!["State".to_string(), truncate_string(&state_str, 100)]);
        } else {
            rows.push(vec!["State".to_string(), "null".to_string()]);
        }

        rows.push(vec!["Events".to_string(), self.events.len().to_string()]);

        if let Some(ref metrics) = self.metrics {
            let metrics_str =
                serde_json::to_string(metrics).unwrap_or_else(|_| "Invalid JSON".to_string());
            rows.push(vec![
                "Metrics".to_string(),
                truncate_string(&metrics_str, 100),
            ]);
        }

        output.table(&headers, &rows)?;
        Ok(())
    }
    fn format_detailed(&self, _output: &OutputManager) -> CliResult<()> {
        todo!()
    }
}

/// Project creation formatter
#[derive(Debug, serde::Serialize)]
pub struct ProjectCreated {
    pub name: String,
    pub template: String,
    pub path: std::path::PathBuf,
    pub build_instructions: Vec<String>,
}

impl OutputFormat for ProjectCreated {
    fn format_compact(&self, output: &OutputManager) -> CliResult<()> {
        println!(
            "{} Created project: {}",
            output.theme().success().apply_to("✓"),
            output.theme().accent().apply_to(&self.name)
        );
        println!(
            "Path: {}",
            output.theme().muted().apply_to(self.path.display())
        );
        Ok(())
    }

    fn format_pretty(&self, output: &OutputManager) -> CliResult<()> {
        println!(
            "{} {}",
            output.theme().success().apply_to("✓"),
            output
                .theme()
                .highlight()
                .apply_to(&format!("Created new actor project: {}", self.name))
        );
        println!();
        println!(
            "Template: {}",
            output.theme().accent().apply_to(&self.template)
        );
        println!(
            "Location: {}",
            output.theme().muted().apply_to(self.path.display())
        );
        println!();
        println!("{}", output.theme().highlight().apply_to("Next steps:"));
        for (i, instruction) in self.build_instructions.iter().enumerate() {
            println!(
                "  {}. {}",
                i + 1,
                output.theme().muted().apply_to(instruction)
            );
        }
        Ok(())
    }

    fn format_table(&self, output: &OutputManager) -> CliResult<()> {
        let headers = vec!["Property", "Value"];
        let rows = vec![
            vec!["Name".to_string(), self.name.clone()],
            vec!["Template".to_string(), self.template.clone()],
            vec!["Path".to_string(), self.path.display().to_string()],
        ];
        output.table(&headers, &rows)?;

        println!();
        println!(
            "{}",
            output.theme().highlight().apply_to("Build Instructions:")
        );
        for (i, instruction) in self.build_instructions.iter().enumerate() {
            println!("  {}. {}", i + 1, instruction);
        }
        Ok(())
    }
    fn format_detailed(&self, _output: &OutputManager) -> CliResult<()> {
        todo!()
    }
}

/// Actor started formatter
#[derive(Debug, serde::Serialize)]
pub struct ActorStarted {
    pub actor_id: String,
    pub manifest_path: String,
    pub address: String,
    pub subscribing: bool,
    pub acting_as_parent: bool,
    pub unix_signals: bool,
}

impl OutputFormat for ActorStarted {
    fn format_compact(&self, output: &OutputManager) -> CliResult<()> {
        println!(
            "{} Actor started: {}",
            output.theme().success_icon(),
            output.theme().accent().apply_to(&self.actor_id)
        );
        Ok(())
    }

    fn format_pretty(&self, output: &OutputManager) -> CliResult<()> {
        println!("{}", "─".repeat(45));
        println!(
            "{} {}",
            output.theme().success_icon(),
            output.theme().highlight().apply_to("ACTOR STARTED")
        );
        println!("{}", "─".repeat(45));
        println!(
            "Actor ID: {}",
            output.theme().accent().apply_to(&self.actor_id)
        );
        println!(
            "Manifest: {}",
            output.theme().muted().apply_to(&self.manifest_path)
        );
        println!("Server: {}", output.theme().muted().apply_to(&self.address));

        if self.subscribing {
            println!(
                "Status: {}",
                output.theme().info().apply_to("Subscribing to events")
            );
        }
        if self.acting_as_parent {
            println!(
                "Role: {}",
                output.theme().info().apply_to("Acting as parent")
            );
        }
        println!("{}", "─".repeat(45));
        Ok(())
    }

    fn format_table(&self, output: &OutputManager) -> CliResult<()> {
        let headers = vec!["Property", "Value"];
        let rows = vec![
            vec!["Actor ID".to_string(), self.actor_id.clone()],
            vec!["Manifest".to_string(), self.manifest_path.clone()],
            vec!["Server".to_string(), self.address.clone()],
            vec!["Subscribing".to_string(), self.subscribing.to_string()],
            vec![
                "Acting as Parent".to_string(),
                self.acting_as_parent.to_string(),
            ],
            vec!["Unix Signals".to_string(), self.unix_signals.to_string()],
        ];
        output.table(&headers, &rows)?;
        Ok(())
    }
    fn format_detailed(&self, _output: &OutputManager) -> CliResult<()> {
        todo!()
    }
}

/// Event subscription formatter
#[derive(Debug, serde::Serialize)]
pub struct EventSubscription {
    pub actor_id: theater::id::TheaterId,
    pub address: String,
    pub event_type_filter: Option<String>,
    pub limit: usize,
    pub timeout: u64,
    pub format: String,
    pub show_history: bool,
    pub history_limit: usize,
    pub detailed: bool,
    pub events_received: usize,
    pub subscription_id: Option<String>,
    pub is_active: bool,
}

impl OutputFormat for EventSubscription {
    fn format_compact(&self, output: &OutputManager) -> CliResult<()> {
        if self.is_active {
            println!(
                "{} Subscribed to: {}",
                output.theme().success_icon(),
                output.theme().accent().apply_to(&self.actor_id.to_string())
            );
            if let Some(filter) = &self.event_type_filter {
                println!("  Filter: {}", output.theme().muted().apply_to(filter));
            }
        } else {
            println!(
                "{} Subscription ended: {} events received",
                output.theme().info_icon(),
                output.theme().accent().apply_to(&self.events_received)
            );
        }
        Ok(())
    }

    fn format_pretty(&self, output: &OutputManager) -> CliResult<()> {
        if self.is_active {
            println!(
                "{} {}",
                output.theme().info_icon(),
                output.theme().highlight().apply_to(&format!(
                    "Subscribing to events for actor: {}",
                    self.actor_id
                ))
            );

            if let Some(filter) = &self.event_type_filter {
                println!(
                    "{} {}",
                    output.theme().info_icon(),
                    output
                        .theme()
                        .highlight()
                        .apply_to(&format!("Filtering events by type: {}", filter))
                );
            }

            println!();
            println!("Server: {}", output.theme().muted().apply_to(&self.address));
            println!("Format: {}", output.theme().muted().apply_to(&self.format));

            if self.limit > 0 {
                println!(
                    "Limit: {} events",
                    output.theme().muted().apply_to(&self.limit)
                );
            }

            if self.timeout > 0 {
                println!(
                    "Timeout: {} seconds",
                    output.theme().muted().apply_to(&self.timeout)
                );
            }

            if self.show_history {
                let history_desc = if self.history_limit > 0 {
                    format!("Last {} events", self.history_limit)
                } else {
                    "All historical events".to_string()
                };
                println!(
                    "History: {}",
                    output.theme().muted().apply_to(&history_desc)
                );
            }

            if let Some(subscription_id) = &self.subscription_id {
                println!(
                    "Subscription ID: {}",
                    output.theme().muted().apply_to(subscription_id)
                );
            }

            println!();
        } else {
            println!("{} Subscription ended", output.theme().success_icon());
            println!(
                "Events received: {}",
                output.theme().accent().apply_to(&self.events_received)
            );
        }
        Ok(())
    }

    fn format_table(&self, output: &OutputManager) -> CliResult<()> {
        let headers = vec!["Property", "Value"];
        let mut rows = vec![
            vec!["Actor ID".to_string(), self.actor_id.to_string()],
            vec!["Server".to_string(), self.address.clone()],
            vec!["Format".to_string(), self.format.clone()],
            vec![
                "Status".to_string(),
                if self.is_active {
                    "Active".to_string()
                } else {
                    "Ended".to_string()
                },
            ],
            vec![
                "Events Received".to_string(),
                self.events_received.to_string(),
            ],
        ];

        if let Some(filter) = &self.event_type_filter {
            rows.push(vec!["Event Filter".to_string(), filter.clone()]);
        }

        if self.limit > 0 {
            rows.push(vec!["Limit".to_string(), self.limit.to_string()]);
        }

        if self.timeout > 0 {
            rows.push(vec![
                "Timeout".to_string(),
                format!("{} seconds", self.timeout),
            ]);
        }

        if let Some(subscription_id) = &self.subscription_id {
            rows.push(vec!["Subscription ID".to_string(), subscription_id.clone()]);
        }

        output.table(&headers, &rows)?;
        Ok(())
    }
    fn format_detailed(&self, _output: &OutputManager) -> CliResult<()> {
        todo!()
    }
}

/// Server started formatter
#[derive(Debug, serde::Serialize)]
pub struct ServerStarted {
    pub address: std::net::SocketAddr,
    pub log_level: String,
    pub log_filter: Option<String>,
    pub log_dir: String,
    pub log_path: std::path::PathBuf,
    pub log_stdout: bool,
}

impl OutputFormat for ServerStarted {
    fn format_compact(&self, output: &OutputManager) -> CliResult<()> {
        println!(
            "{} Theater server starting on {}",
            output.theme().success_icon(),
            output.theme().accent().apply_to(&self.address)
        );
        println!(
            "  Logs: {}",
            output.theme().muted().apply_to(self.log_path.display())
        );
        Ok(())
    }

    fn format_pretty(&self, output: &OutputManager) -> CliResult<()> {
        println!("{}", output.theme().highlight().apply_to("─".repeat(50)));
        println!(
            "{} {}",
            output.theme().success_icon(),
            output
                .theme()
                .highlight()
                .apply_to("THEATER SERVER STARTING")
        );
        println!("{}", output.theme().highlight().apply_to("─".repeat(50)));
        println!();
        println!(
            "Address: {}",
            output.theme().accent().apply_to(&self.address)
        );
        println!(
            "Log Level: {}",
            output.theme().muted().apply_to(&self.log_level)
        );

        if let Some(filter) = &self.log_filter {
            println!("Log Filter: {}", output.theme().muted().apply_to(filter));
        }

        println!(
            "Log Directory: {}",
            output.theme().muted().apply_to(&self.log_dir)
        );
        println!(
            "Log File: {}",
            output.theme().muted().apply_to(self.log_path.display())
        );

        if self.log_stdout {
            println!(
                "Console Logging: {}",
                output.theme().success().apply_to("Enabled")
            );
        }

        println!();
        println!("{}", output.theme().highlight().apply_to("─".repeat(50)));
        println!();
        println!(
            "{} Server is running. Press Ctrl+C to stop.",
            output.theme().info_icon()
        );
        println!();
        Ok(())
    }

    fn format_table(&self, output: &OutputManager) -> CliResult<()> {
        let headers = vec!["Property", "Value"];
        let mut rows = vec![
            vec!["Address".to_string(), self.address.to_string()],
            vec!["Log Level".to_string(), self.log_level.clone()],
            vec!["Log Directory".to_string(), self.log_dir.clone()],
            vec!["Log File".to_string(), self.log_path.display().to_string()],
            vec!["Console Logging".to_string(), self.log_stdout.to_string()],
        ];

        if let Some(filter) = &self.log_filter {
            rows.push(vec!["Custom Filter".to_string(), filter.clone()]);
        }

        output.table(&headers, &rows)?;
        Ok(())
    }
    fn format_detailed(&self, _output: &OutputManager) -> CliResult<()> {
        todo!()
    }
}

/// Channel opened formatter
#[derive(Debug, serde::Serialize)]
pub struct ChannelOpened {
    pub actor_id: theater::id::TheaterId,
    pub channel_id: String,
    pub address: String,
    pub initial_message_size: usize,
    pub is_interactive: bool,
}

impl OutputFormat for ChannelOpened {
    fn format_compact(&self, output: &OutputManager) -> CliResult<()> {
        println!(
            "{} Channel opened to: {}",
            output.theme().success_icon(),
            output.theme().accent().apply_to(&self.actor_id.to_string())
        );
        println!(
            "  Channel ID: {}",
            output.theme().muted().apply_to(&self.channel_id)
        );
        Ok(())
    }

    fn format_pretty(&self, output: &OutputManager) -> CliResult<()> {
        println!(
            "{} {}",
            output.theme().success_icon(),
            output
                .theme()
                .highlight()
                .apply_to(&format!("Opening channel to actor: {}", self.actor_id))
        );
        println!();
        println!(
            "Channel ID: {}",
            output.theme().accent().apply_to(&self.channel_id)
        );
        println!("Server: {}", output.theme().muted().apply_to(&self.address));
        println!(
            "Initial Message: {} bytes",
            output.theme().muted().apply_to(&self.initial_message_size)
        );

        if self.is_interactive {
            println!();
            println!(
                "{} {}",
                output.theme().success_icon(),
                output
                    .theme()
                    .highlight()
                    .apply_to("Channel opened successfully")
            );
        }
        Ok(())
    }

    fn format_table(&self, output: &OutputManager) -> CliResult<()> {
        let headers = vec!["Property", "Value"];
        let rows = vec![
            vec!["Actor ID".to_string(), self.actor_id.to_string()],
            vec!["Channel ID".to_string(), self.channel_id.clone()],
            vec!["Server".to_string(), self.address.clone()],
            vec![
                "Initial Message Size".to_string(),
                format!("{} bytes", self.initial_message_size),
            ],
            vec![
                "Interactive Mode".to_string(),
                self.is_interactive.to_string(),
            ],
        ];
        output.table(&headers, &rows)?;
        Ok(())
    }
    fn format_detailed(&self, _output: &OutputManager) -> CliResult<()> {
        todo!()
    }
}

// Helper functions

fn format_timestamp(timestamp: u64) -> String {
    match DateTime::from_timestamp(timestamp as i64, 0) {
        Some(dt) => dt.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
        None => timestamp.to_string(),
    }
}

fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}…", &s[..max_len.saturating_sub(1)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_string() {
        assert_eq!(truncate_string("hello", 10), "hello");
        assert_eq!(truncate_string("hello world", 5), "hell…");
    }
}

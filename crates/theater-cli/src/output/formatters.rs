use chrono::{DateTime, Utc};
use serde_json::Value;
use theater::ChainEvent;

use crate::error::CliResult;
use crate::output::{OutputFormat, OutputManager};

/// Actor list formatter
#[derive(Debug, serde::Serialize)]
pub struct ActorList {
    pub actors: Vec<(String, String)>,
}

impl OutputFormat for ActorList {
    fn format_compact(&self, output: &OutputManager) -> CliResult<()> {
        if self.actors.is_empty() {
            output.info("No actors are currently running")?;
        } else {
            output.info(&format!("Running actors: {}", self.actors.len()))?;
            for (id, name) in &self.actors {
                println!("  {} {}", 
                    output.theme().accent().apply_to(id),
                    output.theme().muted().apply_to(name)
                );
            }
        }
        Ok(())
    }

    fn format_pretty(&self, output: &OutputManager) -> CliResult<()> {
        if self.actors.is_empty() {
            output.info("No actors are currently running")?;
        } else {
            println!("{}", output.theme().highlight().apply_to("Running Actors"));
            println!("{}", "─".repeat(40));
            
            for (i, (id, name)) in self.actors.iter().enumerate() {
                println!("{}. {} {}", 
                    i + 1,
                    output.theme().accent().apply_to(id),
                    output.theme().muted().apply_to(&format!("({})", name))
                );
            }
            println!();
            output.info(&format!("Total: {} actors", self.actors.len()))?;
        }
        Ok(())
    }

    fn format_table(&self, output: &OutputManager) -> CliResult<()> {
        if self.actors.is_empty() {
            output.info("No actors are currently running")?;
            return Ok(());
        }

        let headers = vec!["ID", "Name"];
        let rows: Vec<Vec<String>> = self.actors
            .iter()
            .map(|(id, name)| vec![id.clone(), name.clone()])
            .collect();

        output.table(&headers, &rows)?;
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

        for event in &self.events {
            let timestamp = format_timestamp(event.timestamp);
            let description = event.description.as_deref().unwrap_or("No description");
            println!("{} {} {}", 
                output.theme().muted().apply_to(&timestamp),
                output.theme().accent().apply_to(&event.event_type),
                truncate_string(description, 60)
            );
        }
        Ok(())
    }

    fn format_pretty(&self, output: &OutputManager) -> CliResult<()> {
        if self.events.is_empty() {
            output.info(&format!("No events found for actor {}", self.actor_id))?;
            return Ok(());
        }

        println!("{}", output.theme().highlight().apply_to(&format!("Events for Actor: {}", self.actor_id)));
        println!("{}", "─".repeat(80));

        for (i, event) in self.events.iter().enumerate() {
            let timestamp = format_timestamp(event.timestamp);
            println!("{}. {} {}", 
                i + 1,
                output.theme().muted().apply_to(&timestamp),
                output.theme().accent().apply_to(&event.event_type)
            );
            let description = event.description.as_deref().unwrap_or("No description");
            println!("   {}", description);
            
            // Note: event.data structure is complex EventData enum, not displaying raw data
            // Could be enhanced to show specific event data based on type
            println!();
        }

        output.info(&format!("Total: {} events", self.events.len()))?;
        Ok(())
    }

    fn format_table(&self, output: &OutputManager) -> CliResult<()> {
        if self.events.is_empty() {
            output.info(&format!("No events found for actor {}", self.actor_id))?;
            return Ok(());
        }

        let headers = vec!["Timestamp", "Type", "Description"];
        let rows: Vec<Vec<String>> = self.events
            .iter()
            .map(|event| vec![
                format_timestamp(event.timestamp),
                event.event_type.clone(),
                truncate_string(event.description.as_deref().unwrap_or("No description"), 50),
            ])
            .collect();

        output.table(&headers, &rows)?;
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
        println!("State for actor {}: {}", 
            output.theme().accent().apply_to(&self.actor_id),
            self.state
        );
        Ok(())
    }

    fn format_pretty(&self, output: &OutputManager) -> CliResult<()> {
        println!("{}", output.theme().highlight().apply_to(&format!("State for Actor: {}", self.actor_id)));
        println!("{}", "─".repeat(40));
        println!("{}", serde_json::to_string_pretty(&self.state).unwrap_or_else(|_| "Invalid JSON".to_string()));
        Ok(())
    }

    fn format_table(&self, output: &OutputManager) -> CliResult<()> {
        // For state, table format doesn't make much sense, so fall back to pretty
        self.format_pretty(output)
    }
}

/// Build result formatter
#[derive(Debug, serde::Serialize)]
pub struct BuildResult {
    pub success: bool,
    pub output: String,
    pub component_path: Option<String>,
}

impl OutputFormat for BuildResult {
    fn format_compact(&self, output: &OutputManager) -> CliResult<()> {
        if self.success {
            output.success("Build completed successfully")?;
            if let Some(path) = &self.component_path {
                println!("  Component: {}", output.theme().accent().apply_to(path));
            }
        } else {
            output.error("Build failed")?;
            if !self.output.is_empty() {
                println!("{}", output.theme().muted().apply_to(&self.output));
            }
        }
        Ok(())
    }

    fn format_pretty(&self, output: &OutputManager) -> CliResult<()> {
        if self.success {
            println!("{} {}", 
                output.theme().success_icon(),
                output.theme().highlight().apply_to("Build Successful")
            );
            
            if let Some(path) = &self.component_path {
                println!("Component: {}", output.theme().accent().apply_to(path));
            }
            
            if !self.output.is_empty() {
                println!("\nBuild Output:");
                println!("{}", output.theme().muted().apply_to(&self.output));
            }
        } else {
            println!("{} {}", 
                output.theme().error_icon(),
                output.theme().error().apply_to("Build Failed")
            );
            
            if !self.output.is_empty() {
                println!("\nError Output:");
                println!("{}", self.output);
            }
        }
        Ok(())
    }

    fn format_table(&self, output: &OutputManager) -> CliResult<()> {
        let headers = vec!["Property", "Value"];
        let mut rows = vec![
            vec!["Status".to_string(), if self.success { "Success".to_string() } else { "Failed".to_string() }],
        ];
        
        if let Some(path) = &self.component_path {
            rows.push(vec!["Component".to_string(), path.clone()]);
        }
        
        if !self.output.is_empty() {
            rows.push(vec!["Output".to_string(), truncate_string(&self.output, 100)]);
        }
        
        output.table(&headers, &rows)?;
        Ok(())
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
            println!("{} {} actor: {}", 
                output.theme().success_icon(),
                self.action.chars().next().unwrap().to_uppercase().collect::<String>() + &self.action[1..],
                output.theme().accent().apply_to(&self.actor_id)
            );
        } else {
            println!("{} Failed to {} actor: {}", 
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
            println!("{} {}", 
                output.theme().success_icon(),
                output.theme().highlight().apply_to(&format!(
                    "Actor {} Successfully", 
                    self.action.chars().next().unwrap().to_uppercase().collect::<String>() + &self.action[1..]
                ))
            );
            println!("Actor ID: {}", output.theme().accent().apply_to(&self.actor_id));
        } else {
            println!("{} {}", 
                output.theme().error_icon(),
                output.theme().error().apply_to(&format!("Failed to {} Actor", self.action))
            );
            println!("Actor ID: {}", output.theme().accent().apply_to(&self.actor_id));
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
            vec!["Status".to_string(), if self.success { "Success".to_string() } else { "Failed".to_string() }],
        ];
        
        if let Some(msg) = &self.message {
            rows.push(vec!["Message".to_string(), msg.clone()]);
        }
        
        output.table(&headers, &rows)?;
        Ok(())
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
            println!("{} Updated actor: {} with component: {}", 
                output.theme().success_icon(),
                output.theme().accent().apply_to(&self.actor_id),
                output.theme().accent().apply_to(&self.component)
            );
        } else {
            println!("{} Failed to update actor: {}", 
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
            println!("{} {}", 
                output.theme().success_icon(),
                output.theme().highlight().apply_to("Component Updated Successfully")
            );
            println!("Actor ID: {}", output.theme().accent().apply_to(&self.actor_id));
            println!("New Component: {}", output.theme().accent().apply_to(&self.component));
        } else {
            println!("{} {}", 
                output.theme().error_icon(),
                output.theme().error().apply_to("Failed to Update Component")
            );
            println!("Actor ID: {}", output.theme().accent().apply_to(&self.actor_id));
            println!("Component: {}", output.theme().accent().apply_to(&self.component));
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
            vec!["Status".to_string(), if self.success { "Success".to_string() } else { "Failed".to_string() }],
        ];
        
        if let Some(msg) = &self.message {
            rows.push(vec!["Message".to_string(), msg.clone()]);
        }
        
        output.table(&headers, &rows)?;
        Ok(())
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
            println!("{} Message sent to actor: {}", 
                output.theme().success_icon(),
                output.theme().accent().apply_to(&self.actor_id)
            );
        } else {
            println!("{} Failed to send message to actor: {}", 
                output.theme().error_icon(),
                output.theme().accent().apply_to(&self.actor_id)
            );
        }
        Ok(())
    }

    fn format_pretty(&self, output: &OutputManager) -> CliResult<()> {
        if self.success {
            println!("{} {}", 
                output.theme().success_icon(),
                output.theme().highlight().apply_to("Message Sent Successfully")
            );
            println!("Actor ID: {}", output.theme().accent().apply_to(&self.actor_id));
            println!("Message: {}", output.theme().muted().apply_to(&truncate_string(&self.message, 100)));
        } else {
            println!("{} {}", 
                output.theme().error_icon(),
                output.theme().error().apply_to("Failed to Send Message")
            );
            println!("Actor ID: {}", output.theme().accent().apply_to(&self.actor_id));
        }
        Ok(())
    }

    fn format_table(&self, output: &OutputManager) -> CliResult<()> {
        let headers = vec!["Property", "Value"];
        let rows = vec![
            vec!["Actor ID".to_string(), self.actor_id.clone()],
            vec!["Message".to_string(), truncate_string(&self.message, 100)],
            vec!["Status".to_string(), if self.success { "Sent".to_string() } else { "Failed".to_string() }],
        ];
        
        output.table(&headers, &rows)?;
        Ok(())
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
        println!("{} Response from actor: {}", 
            output.theme().success_icon(),
            output.theme().accent().apply_to(&self.actor_id)
        );
        println!("{}", self.response);
        Ok(())
    }

    fn format_pretty(&self, output: &OutputManager) -> CliResult<()> {
        println!("{} {}", 
            output.theme().success_icon(),
            output.theme().highlight().apply_to("Response Received")
        );
        println!("Actor ID: {}", output.theme().accent().apply_to(&self.actor_id));
        println!("Request: {}", output.theme().muted().apply_to(&truncate_string(&self.request, 100)));
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
            output.info(&format!("No stored actors found. Chains directory does not exist: {}", self.chains_dir))?;
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
            println!("{} {}", 
                output.theme().info_icon(),
                output.theme().highlight().apply_to("No Stored Actors Found")
            );
            println!("Chains directory does not exist: {}", 
                output.theme().muted().apply_to(&self.chains_dir));
        } else {
            println!("{} {}", 
                output.theme().info_icon(),
                output.theme().highlight().apply_to(&format!("Stored Actors ({})", self.actor_ids.len()))
            );
            println!("Directory: {}", output.theme().muted().apply_to(&self.chains_dir));
            println!("{}", "─".repeat(40));
            
            if self.actor_ids.is_empty() {
                println!("No stored actors found.");
            } else {
                for (i, actor_id) in self.actor_ids.iter().enumerate() {
                    println!("{}. {}", 
                        i + 1,
                        output.theme().accent().apply_to(actor_id)
                    );
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
        let rows: Vec<Vec<String>> = self.actor_ids
            .iter()
            .enumerate()
            .map(|(i, id)| vec![(i + 1).to_string(), id.clone()])
            .collect();

        output.table(&headers, &rows)?;
        Ok(())
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
        println!("{} Logs for actor: {}", 
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
        println!("{} {}", 
            output.theme().info_icon(),
            output.theme().highlight().apply_to(&format!("Logs for Actor: {}", self.actor_id))
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
                        println!("{} {}", 
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
        let rows: Vec<Vec<String>> = self.events
            .iter()
            .filter_map(|event| {
                serde_json::from_slice::<serde_json::Value>(&event.data)
                    .ok()
                    .and_then(|json_data| {
                        json_data.get("message")
                            .and_then(|m| m.as_str())
                            .map(|message| vec![
                                format_timestamp(event.timestamp),
                                truncate_string(message, 80),
                            ])
                    })
            })
            .collect();

        output.table(&headers, &rows)?;
        Ok(())
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
            println!("Theater Server {}", output.theme().accent().apply_to(version));
        }
        if let Some(uptime) = self.info.get("uptime") {
            println!("Uptime: {}", output.theme().muted().apply_to(uptime));
        }
        Ok(())
    }

    fn format_pretty(&self, output: &OutputManager) -> CliResult<()> {
        println!("{}", output.theme().highlight().apply_to("Server Information"));
        println!("{}", "─".repeat(40));
        println!("{}", serde_json::to_string_pretty(&self.info).unwrap_or_else(|_| "Invalid JSON".to_string()));
        Ok(())
    }

    fn format_table(&self, output: &OutputManager) -> CliResult<()> {
        // Convert JSON object to key-value table
        if let Value::Object(map) = &self.info {
            let headers = vec!["Property", "Value"];
            let rows: Vec<Vec<String>> = map
                .iter()
                .map(|(key, value)| vec![
                    key.clone(),
                    match value {
                        Value::String(s) => s.clone(),
                        _ => value.to_string(),
                    }
                ])
                .collect();
            output.table(&headers, &rows)?;
        } else {
            self.format_pretty(output)?;
        }
        Ok(())
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
        println!("{} - {}", 
            output.theme().accent().apply_to(&self.id.to_string()),
            output.theme().muted().apply_to(&self.status)
        );
        if let Some(ref state) = self.state {
            let state_str = serde_json::to_string(state).unwrap_or_else(|_| "Invalid JSON".to_string());
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
        
        println!("{}", output.theme().highlight().apply_to("ACTOR INFORMATION"));
        println!("{}", "─".repeat(50));
        println!("ID: {}", output.theme().accent().apply_to(&self.id.to_string()));
        println!("Status: {}", output.theme().muted().apply_to(&self.status));
        
        // Calculate uptime if we have events
        if let Some(first_event) = self.events.first() {
            let now = chrono::Utc::now().timestamp() as u64;
            let uptime = Duration::from_secs(now.saturating_sub(first_event.timestamp));
            println!("Uptime: {}", crate::utils::formatting::format_duration(uptime));
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
                println!("{}. {}", i + 1, crate::utils::formatting::format_event_summary(event));
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
            println!("{}", serde_json::to_string_pretty(metrics)
                .unwrap_or_else(|_| "Invalid JSON".to_string()));
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
            let state_str = serde_json::to_string(state)
                .unwrap_or_else(|_| "Invalid JSON".to_string());
            rows.push(vec!["State".to_string(), truncate_string(&state_str, 100)]);
        } else {
            rows.push(vec!["State".to_string(), "null".to_string()]);
        }
        
        rows.push(vec!["Events".to_string(), self.events.len().to_string()]);
        
        if let Some(ref metrics) = self.metrics {
            let metrics_str = serde_json::to_string(metrics)
                .unwrap_or_else(|_| "Invalid JSON".to_string());
            rows.push(vec!["Metrics".to_string(), truncate_string(&metrics_str, 100)]);
        }
        
        output.table(&headers, &rows)?;
        Ok(())
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

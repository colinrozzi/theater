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
            println!("{}", output.theme().highlight().apply_to("Build Results"));
            println!("{}", "─".repeat(40));
            output.success("Build completed successfully")?;
            
            if let Some(path) = &self.component_path {
                println!("Component: {}", output.theme().accent().apply_to(path));
            }
            
            if !self.output.is_empty() {
                println!("\nBuild Output:");
                println!("{}", output.theme().muted().apply_to(&self.output));
            }
        } else {
            output.error("Build failed")?;
            println!("\nBuild Output:");
            println!("{}", self.output);
        }
        Ok(())
    }

    fn format_table(&self, output: &OutputManager) -> CliResult<()> {
        let headers = vec!["Status", "Component", "Output"];
        let status = if self.success { "Success" } else { "Failed" };
        let component = self.component_path.as_deref().unwrap_or("N/A");
        let build_output = truncate_string(&self.output, 50);
        
        let rows = vec![vec![status.to_string(), component.to_string(), build_output]];
        output.table(&headers, &rows)?;
        Ok(())
    }
}

/// Server info formatter
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

    #[test]
    fn test_format_timestamp() {
        let timestamp = 1609459200; // 2021-01-01 00:00:00 UTC
        let formatted = format_timestamp(timestamp);
        assert!(formatted.contains("2021-01-01"));
    }
}

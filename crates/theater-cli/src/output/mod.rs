mod formatters;
mod progress;
mod theme;

pub use formatters::*;
pub use progress::*;
pub use theme::*;

use console::{style, Term};
use serde_json::Value;
use std::io::Write;

use crate::config::OutputConfig;
use crate::error::CliResult;

/// Main output handler for the CLI
#[derive(Debug)]
pub struct OutputManager {
    config: OutputConfig,
    term: Term,
    theme: Theme,
}

impl OutputManager {
    pub fn new(config: OutputConfig) -> Self {
        let term = Term::stdout();
        let theme = if config.colors && term.features().colors_supported() {
            Theme::colored()
        } else {
            Theme::plain()
        };

        Self {
            config,
            term,
            theme,
        }
    }

    /// Print a success message
    pub fn success(&self, message: &str) -> CliResult<()> {
        println!("{} {}", self.theme.success_icon(), message);
        Ok(())
    }

    /// Print an error message
    pub fn error(&self, message: &str) -> CliResult<()> {
        eprintln!("{} {}", self.theme.error_icon(), message);
        Ok(())
    }

    /// Print a warning message
    pub fn warning(&self, message: &str) -> CliResult<()> {
        println!("{} {}", self.theme.warning_icon(), message);
        Ok(())
    }

    /// Print an info message
    pub fn info(&self, message: &str) -> CliResult<()> {
        println!("{} {}", self.theme.info_icon(), message);
        Ok(())
    }

    /// Print formatted output based on the configured format
    pub fn output<T>(&self, data: &T, format: Option<&str>) -> CliResult<()>
    where
        T: serde::Serialize + OutputFormat,
    {
        let format = format.unwrap_or(&self.config.default_format);

        match format {
            "json" => {
                let json = serde_json::to_string_pretty(data)
                    .map_err(|e| crate::error::CliError::Serialization(e))?;
                println!("{}", json);
            }
            "yaml" => {
                let yaml = serde_yaml::to_string(data)
                    .map_err(|e| crate::error::CliError::Internal(e.into()))?;
                println!("{}", yaml);
            }
            "compact" => {
                data.format_compact(self)?;
            }
            "pretty" => {
                data.format_pretty(self)?;
            }
            "table" => {
                data.format_table(self)?;
            }
            _ => {
                return Err(crate::error::CliError::invalid_input(
                    "format",
                    format,
                    "Supported formats: json, yaml, compact, pretty, table",
                ));
            }
        }

        Ok(())
    }

    /// Get the terminal
    pub fn term(&self) -> &Term {
        &self.term
    }

    /// Get the theme
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Get the output configuration
    pub fn config(&self) -> &OutputConfig {
        &self.config
    }

    /// Create a progress bar
    pub fn progress_bar(&self, len: u64) -> ProgressBar {
        ProgressBar::new(len, self.theme.clone())
    }

    /// Print a table with headers and rows
    pub fn table(&self, headers: &[&str], rows: &[Vec<String>]) -> CliResult<()> {
        if rows.is_empty() {
            return Ok(());
        }

        // Calculate column widths
        let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
        
        for row in rows {
            for (i, cell) in row.iter().enumerate() {
                if i < widths.len() {
                    widths[i] = widths[i].max(cell.len());
                }
            }
        }

        // Apply max width if configured
        if let Some(max_width) = self.config.max_width {
            let available_width = max_width.saturating_sub(widths.len() * 3); // Account for separators
            let total_width: usize = widths.iter().sum();
            
            if total_width > available_width {
                // Proportionally reduce column widths
                let scale = available_width as f64 / total_width as f64;
                for width in &mut widths {
                    *width = (*width as f64 * scale) as usize;
                }
            }
        }

        // Print header
        print!("│");
        for (i, header) in headers.iter().enumerate() {
            print!(" {:width$} │", 
                self.theme.table_header().apply_to(header), 
                width = widths[i]
            );
        }
        println!();

        // Print separator
        print!("├");
        for width in &widths {
            print!("{}", "─".repeat(width + 2));
            print!("┼");
        }
        println!();

        // Print rows
        for row in rows {
            print!("│");
            for (i, cell) in row.iter().enumerate() {
                let truncated = if i < widths.len() && cell.len() > widths[i] {
                    format!("{}…", &cell[..widths[i].saturating_sub(1)])
                } else {
                    cell.clone()
                };
                
                print!(" {:width$} │", truncated, width = widths[i]);
            }
            println!();
        }

        Ok(())
    }
}

/// Trait for types that can be formatted in different ways
pub trait OutputFormat {
    fn format_compact(&self, output: &OutputManager) -> CliResult<()>;
    fn format_pretty(&self, output: &OutputManager) -> CliResult<()>;
    fn format_table(&self, output: &OutputManager) -> CliResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::OutputConfig;

    #[test]
    fn test_output_manager_creation() {
        let config = OutputConfig {
            default_format: "json".to_string(),
            colors: true,
            timestamps: true,
            max_width: Some(100),
        };
        
        let output = OutputManager::new(config);
        assert_eq!(output.config().default_format, "json");
    }
}

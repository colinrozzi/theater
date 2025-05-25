use console::style;
use std::time::Duration;
use theater::id::TheaterId;
use theater::messages::ActorStatus;
use theater::ChainEvent;

/// Format an actor ID in a consistent way
pub fn format_id(id: &TheaterId) -> String {
    style(id.to_string()).cyan().to_string()
}

/// Format a short version of an actor ID (first 8 chars)
pub fn format_short_id(id: &TheaterId) -> String {
    let id_str = id.to_string();
    let short_id = &id_str[..std::cmp::min(8, id_str.len())];
    style(short_id).cyan().to_string()
}

/// Format an actor status with appropriate color
pub fn format_status(status: &ActorStatus) -> String {
    match status {
        ActorStatus::Running => style("RUNNING").green().bold().to_string(),
        ActorStatus::Stopped => style("STOPPED").red().bold().to_string(),
        ActorStatus::Failed => style("FAILED").red().bold().to_string(),
    }
}

/// Format a timestamp as a human-readable date/time
pub fn format_timestamp(timestamp: &u64) -> String {
    let datetime = chrono::DateTime::from_timestamp(*timestamp as i64, 0)
        .unwrap_or_else(|| chrono::DateTime::UNIX_EPOCH);
    datetime.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Format a duration in a human-readable form
pub fn format_duration(duration: Duration) -> String {
    let total_secs = duration.as_secs();

    if total_secs < 60 {
        return format!("{}s", total_secs);
    }

    let mins = total_secs / 60;
    let secs = total_secs % 60;

    if mins < 60 {
        return format!("{}m {}s", mins, secs);
    }

    let hours = mins / 60;
    let mins = mins % 60;

    if hours < 24 {
        return format!("{}h {}m {}s", hours, mins, secs);
    }

    let days = hours / 24;
    let hours = hours % 24;

    format!("{}d {}h {}m {}s", days, hours, mins, secs)
}

/// Format a byte array as a hex string with optional shortening
pub fn format_hash(hash: &[u8], shorten: bool) -> String {
    let hex = hex::encode(hash);
    if shorten && hex.len() > 16 {
        format!("{}..{}", &hex[0..8], &hex[hex.len() - 8..])
    } else {
        hex
    }
}

/// Format a section header
pub fn format_section(title: &str) -> String {
    format!(
        "\n{}\n{}",
        style(title).bold().underlined(),
        style("─".repeat(title.len())).dim()
    )
}

/// Format a key-value pair for display
pub fn format_key_value(key: &str, value: &str) -> String {
    format!("{}: {}", style(key).bold(), value)
}

/// Format an event summary
pub fn format_event_summary(event: &ChainEvent) -> String {
    let event_type = style(&event.event_type).yellow();
    let timestamp = format_timestamp(&event.timestamp);
    let hash = format_hash(&event.hash, true);

    format!(
        "{} at {} (hash: {})",
        event_type,
        style(timestamp).dim(),
        style(hash).dim()
    )
}

/// Create a table with headers and rows
#[allow(dead_code)]
pub fn format_table(headers: &[&str], rows: &[Vec<String>], indent: usize) -> String {
    if rows.is_empty() {
        return "No data available".to_string();
    }

    // Calculate column widths
    let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();

    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < widths.len() {
                widths[i] = std::cmp::max(widths[i], cell.len());
            }
        }
    }

    // Format the header
    let mut result = " ".repeat(indent);
    for (i, header) in headers.iter().enumerate() {
        result.push_str(&format!(
            "{:<width$} ",
            style(*header).bold(),
            width = widths[i]
        ));
    }
    result.push('\n');

    // Add separator
    result.push_str(&" ".repeat(indent));
    for width in &widths {
        result.push_str(&style("─".repeat(*width)).dim().to_string());
        result.push(' ');
    }
    result.push('\n');

    // Format rows
    for row in rows {
        result.push_str(&" ".repeat(indent));
        for (i, cell) in row.iter().enumerate() {
            if i < widths.len() {
                result.push_str(&format!("{:<width$} ", cell, width = widths[i]));
            }
        }
        result.push('\n');
    }

    result
}

/// Format a success message
pub fn format_success(message: &str) -> String {
    format!("{} {}", style("✓").green().bold(), message)
}

/// Format an error message
pub fn format_error(message: &str) -> String {
    format!("{} {}", style("✗").red().bold(), message)
}

/// Format a warning message
pub fn format_info(message: &str) -> String {
    format!("{} {}", style("ℹ").blue().bold(), message)
}

/// Format a warning message
pub fn format_warning(message: &str) -> String {
    format!("{} {}", style("⚠").yellow().bold(), message)
}

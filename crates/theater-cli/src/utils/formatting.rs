use console::style;

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

/// Format a success message
pub fn format_success(message: &str) -> String {
    format!("{} {}", style("✓").green().bold(), message)
}

/// Format an error message
pub fn format_error(message: &str) -> String {
    format!("{} {}", style("✗").red().bold(), message)
}

/// Format an info message
pub fn format_info(message: &str) -> String {
    format!("{} {}", style("ℹ").blue().bold(), message)
}

/// Format a warning message
pub fn format_warning(message: &str) -> String {
    format!("{} {}", style("⚠").yellow().bold(), message)
}

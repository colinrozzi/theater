use console::Style;

/// Theme for consistent CLI styling
#[derive(Debug, Clone)]
pub struct Theme {
    pub success: Style,
    pub error: Style,
    pub warning: Style,
    pub info: Style,
    pub accent: Style,
    pub muted: Style,
    pub highlight: Style,
    pub table_header: Style,
}

impl Theme {
    /// Create a colored theme
    pub fn colored() -> Self {
        Self {
            success: Style::new().green().bold(),
            error: Style::new().red().bold(),
            warning: Style::new().yellow().bold(),
            info: Style::new().blue().bold(),
            accent: Style::new().cyan(),
            muted: Style::new().dim(),
            highlight: Style::new().bright().bold(),
            table_header: Style::new().bold().underlined(),
        }
    }

    /// Create a plain theme (no colors)
    pub fn plain() -> Self {
        Self {
            success: Style::new(),
            error: Style::new(),
            warning: Style::new(),
            info: Style::new(),
            accent: Style::new(),
            muted: Style::new(),
            highlight: Style::new(),
            table_header: Style::new(),
        }
    }

    // Icon methods
    pub fn success_icon(&self) -> console::StyledObject<&str> {
        self.success.apply_to("✓")
    }

    pub fn error_icon(&self) -> console::StyledObject<&str> {
        self.error.apply_to("✗")
    }

    pub fn warning_icon(&self) -> console::StyledObject<&str> {
        self.warning.apply_to("⚠")
    }

    pub fn info_icon(&self) -> console::StyledObject<&str> {
        self.info.apply_to("ℹ")
    }

    // Style accessors
    pub fn success(&self) -> &Style {
        &self.success
    }

    pub fn error(&self) -> &Style {
        &self.error
    }

    pub fn warning(&self) -> &Style {
        &self.warning
    }

    pub fn info(&self) -> &Style {
        &self.info
    }

    pub fn accent(&self) -> &Style {
        &self.accent
    }

    pub fn muted(&self) -> &Style {
        &self.muted
    }

    pub fn highlight(&self) -> &Style {
        &self.highlight
    }

    pub fn table_header(&self) -> &Style {
        &self.table_header
    }
}

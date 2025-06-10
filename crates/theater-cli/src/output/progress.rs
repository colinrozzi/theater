use indicatif::{ProgressBar as IndicatifBar, ProgressStyle};
use std::time::Duration;

use crate::output::Theme;

/// A progress bar wrapper with consistent styling
#[derive(Debug)]
pub struct ProgressBar {
    bar: IndicatifBar,
    theme: Theme,
}

impl ProgressBar {
    pub fn new(len: u64, theme: Theme) -> Self {
        let bar = IndicatifBar::new(len);

        // Set a nice style
        let style = ProgressStyle::default_bar()
            .template(
                "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>7}/{len:7} {msg}",
            )
            .unwrap()
            .progress_chars("██░");

        bar.set_style(style);
        bar.enable_steady_tick(Duration::from_millis(100));

        Self { bar, theme }
    }

    /// Create an indeterminate progress bar (spinner)
    pub fn new_spinner(theme: Theme) -> Self {
        let bar = IndicatifBar::new_spinner();

        let style = ProgressStyle::default_spinner()
            .template("{spinner:.green} {elapsed_precise} {msg}")
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]);

        bar.set_style(style);
        bar.enable_steady_tick(Duration::from_millis(100));

        Self { bar, theme }
    }

    /// Set the current position
    pub fn set_position(&self, pos: u64) {
        self.bar.set_position(pos);
    }

    /// Increment the position
    pub fn inc(&self, delta: u64) {
        self.bar.inc(delta);
    }

    /// Set the message
    pub fn set_message(&self, message: &str) {
        self.bar.set_message(message.to_string());
    }

    /// Finish the progress bar with a message
    pub fn finish_with_message(&self, message: &str) {
        self.bar.finish_with_message(message.to_string());
    }

    /// Finish the progress bar
    pub fn finish(&self) {
        self.bar.finish();
    }

    /// Abandon the progress bar (useful for error cases)
    pub fn abandon(&self) {
        self.bar.abandon();
    }

    /// Check if the progress bar is finished
    pub fn is_finished(&self) -> bool {
        self.bar.is_finished()
    }
}

impl Drop for ProgressBar {
    fn drop(&mut self) {
        if !self.is_finished() {
            self.abandon();
        }
    }
}

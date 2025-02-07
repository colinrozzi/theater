use anyhow::Result;
use std::fs::{self, File};
use std::path::Path;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

pub fn setup_global_logging(
    log_path: impl AsRef<Path>,
    filter_string: &str,
    with_stdout: bool,
) -> Result<()> {
    let log_path = log_path.as_ref();

    // Ensure parent directory exists
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let file = File::create(log_path)?;
    let file_writer = std::sync::Mutex::new(file).with_max_level(tracing::Level::TRACE);

    let file_layer = fmt::layer()
        .with_writer(file_writer)
        .with_thread_ids(true)
        .with_line_number(true)
        .with_file(true)
        .with_target(true)
        .with_ansi(false)
        .with_filter(EnvFilter::builder().parse(filter_string)?);

    if with_stdout {
        let stdout_layer = fmt::layer()
            .with_writer(std::io::stdout)
            .with_thread_ids(true)
            .with_line_number(true)
            .with_file(true)
            .with_target(true)
            .with_ansi(true)
            .with_filter(EnvFilter::builder().parse(filter_string)?);

        tracing_subscriber::registry()
            .with(file_layer)
            .with(stdout_layer)
            .try_init()
            .map_err(|e| anyhow::anyhow!("Failed to initialize logging: {}", e))?;
    } else {
        tracing_subscriber::registry()
            .with(file_layer)
            .try_init()
            .map_err(|e| anyhow::anyhow!("Failed to initialize logging: {}", e))?;
    }

    Ok(())
}

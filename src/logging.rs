use anyhow::Result;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use tracing_subscriber::{
    fmt::{self, format::Writer, time::FormatTime, FmtContext, FormatEvent, FormatFields},
    prelude::*,
    registry::LookupSpan,
    EnvFilter,
};

// Custom formatter
struct CustomFormat;

impl<S, N> FormatEvent<S, N> for CustomFormat
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> std::fmt::Result {
        // Write timestamp
        let timestamp = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3fZ");
        write!(writer, "{} ", timestamp)?;

        // Write level
        if let Some(level) = event.metadata().level() {
            write!(writer, "{:<5} ", level)?;
        }

        // Write actor information from spans
        if let Some(scope) = ctx.event_scope() {
            for span in scope.from_root() {
                let extensions = span.extensions();

                if let Some(actor_id) = span.fields().field("actor_id") {
                    write!(writer, "[actor_id={}] ", actor_id)?;
                }
                if let Some(actor_name) = span.fields().field("actor_name") {
                    write!(writer, "[actor_name={}] ", actor_name)?;
                }
            }
        }

        // Write target and message
        write!(writer, "{}: ", event.metadata().target())?;
        ctx.format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

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
        .event_format(CustomFormat)
        .with_filter(EnvFilter::builder().parse(filter_string)?);

    if with_stdout {
        let stdout_layer = fmt::layer()
            .with_writer(std::io::stdout)
            .with_thread_ids(true)
            .with_line_number(true)
            .with_file(true)
            .with_target(true)
            .event_format(CustomFormat)
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

pub fn olf_setup_file_logging(
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

    let format = tracing_subscriber::fmt::format()
        .with_level(true)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_line_number(true)
        .with_target(true)
        .format_event(|writer, event| {
            // Write timestamp
            let timestamp = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3fZ");
            write!(writer, "{} ", timestamp)?;

            // Write level
            if let Some(level) = event.metadata().level() {
                write!(writer, "{:<5} ", level)?;
            }

            // Write span information (actor details)
            if let Some(scope) = event.scope() {
                for span in scope.from_root() {
                    let extensions = span.extensions();
                    if let Some(actor_id) = extensions.get::<&str>("actor_id") {
                        write!(writer, "[actor_id={}] ", actor_id)?;
                    }
                    if let Some(actor_name) = extensions.get::<&str>("actor_name") {
                        write!(writer, "[actor_name={}] ", actor_name)?;
                    }
                }
            }

            // Write the actual message
            if let Some(message) = event.message() {
                write!(writer, "{}", message)?;
            }

            writeln!(writer)
        });

    let file_layer = fmt::layer()
        .with_writer(file_writer)
        //.event_format(format)
        .with_filter(EnvFilter::builder().parse(filter_string)?);

    if with_stdout {
        let stdout_layer = fmt::layer()
            .with_writer(std::io::stdout)
            //.event_format(format)
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

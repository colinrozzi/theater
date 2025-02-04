use anyhow::Result;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use tracing_subscriber::{
    fmt::{self, format::Writer, time::FormatTime, FmtContext, FormatEvent, FormatFields},
    prelude::*,
    registry::LookupSpan,
    EnvFilter,
};

struct CustomFormat {
    with_ansi: bool,
}

impl CustomFormat {
    fn new(with_ansi: bool) -> Self {
        Self { with_ansi }
    }

    fn level_color(&self, level: &tracing::Level) -> &str {
        if !self.with_ansi {
            return "";
        }
        match *level {
            tracing::Level::TRACE => "\x1b[34m", // Blue
            tracing::Level::DEBUG => "\x1b[36m", // Cyan
            tracing::Level::INFO => "\x1b[32m",  // Green
            tracing::Level::WARN => "\x1b[33m",  // Yellow
            tracing::Level::ERROR => "\x1b[31m", // Red
        }
    }

    fn reset_color(&self) -> &str {
        if self.with_ansi {
            "\x1b[0m"
        } else {
            ""
        }
    }
}

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

        // Write level with color
        let level = event.metadata().level();
        write!(
            writer,
            "{}{:<5}{} ",
            self.level_color(level),
            level,
            self.reset_color()
        )?;

        // Write actor information from spans
        if let Some(scope) = ctx.event_scope() {
            for span in scope.from_root() {
                // Use visit_fields to properly access the span's fields
                let mut visitor = SpanFieldVisitor::default();
                span.record(&mut visitor);

                if let Some(actor_id) = visitor.actor_id {
                    write!(writer, "[actor_id={}] ", actor_id)?;
                }
                if let Some(actor_name) = visitor.actor_name {
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

// Helper struct to collect span fields
#[derive(Default)]
struct SpanFieldVisitor {
    actor_id: Option<String>,
    actor_name: Option<String>,
}

impl tracing::field::Visit for SpanFieldVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        match field.name() {
            "actor_id" => self.actor_id = Some(format!("{:?}", value)),
            "actor_name" => self.actor_name = Some(format!("{:?}", value)),
            _ => {}
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        match field.name() {
            "actor_id" => self.actor_id = Some(value.to_string()),
            "actor_name" => self.actor_name = Some(value.to_string()),
            _ => {}
        }
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
        .event_format(CustomFormat::new(false)) // No colors in file
        .with_filter(EnvFilter::builder().parse(filter_string)?);

    if with_stdout {
        let stdout_layer = fmt::layer()
            .with_writer(std::io::stdout)
            .with_thread_ids(true)
            .with_line_number(true)
            .with_file(true)
            .with_target(true)
            .event_format(CustomFormat::new(true)) // Colors in stdout
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

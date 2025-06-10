use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::tui::app::{ActorStatus, LifecycleEvent, LifecycleEventType, TuiApp};

pub fn render_status_panel(f: &mut Frame, app: &TuiApp, area: Rect) {
    let block = Block::default()
        .title(" Lifecycle & Status ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green));

    // Split the panel into sections
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6), // Actor info
            Constraint::Length(4), // Status
            Constraint::Length(3), // Stats
            Constraint::Min(3),    // Recent lifecycle events
        ])
        .split(Rect {
            x: area.x + 1,
            y: area.y + 1,
            width: area.width - 2,
            height: area.height - 2,
        });

    // Render the outer block
    f.render_widget(block, area);

    // Actor Info Section
    render_actor_info(f, app, chunks[0]);

    // Status Section
    render_status_info(f, app, chunks[1]);

    // Stats Section
    render_stats(f, app, chunks[2]);

    // Recent Events Section
    render_recent_lifecycle_events(f, app, chunks[3]);
}

fn render_actor_info(f: &mut Frame, app: &TuiApp, area: Rect) {
    let status_symbol = match app.current_status {
        ActorStatus::Starting => "🟡",
        ActorStatus::Running => "✅",
        ActorStatus::Paused => "⏸️",
        ActorStatus::Stopped => "⏹️",
        ActorStatus::Error => "❌",
    };

    let status_text = format!("{:?}", app.current_status);
    let runtime = format_duration(app.start_time);

    let lines = vec![
        Line::from(vec![
            Span::styled(status_symbol, Style::default()),
            Span::styled(
                format!(" Actor {}", status_text),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("   ID: ", Style::default().fg(Color::Gray)),
            Span::styled(&app.actor_id, Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::styled("   Manifest: ", Style::default().fg(Color::Gray)),
            Span::styled(&app.manifest_path, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("   Runtime: ", Style::default().fg(Color::Gray)),
            Span::styled(runtime, Style::default().fg(Color::Green)),
        ]),
        Line::from(""),
    ];

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, area);
}

fn render_status_info(f: &mut Frame, app: &TuiApp, area: Rect) {
    let last_event_time = app
        .events
        .back()
        .map(|e| e.timestamp.format("%H:%M:%S").to_string())
        .unwrap_or_else(|| "Never".to_string());

    let lines = vec![
        Line::from(vec![Span::styled(
            "🟡 Recent Activity",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled("   Last Event: ", Style::default().fg(Color::Gray)),
            Span::styled(last_event_time, Style::default().fg(Color::White)),
        ]),
        Line::from(""),
    ];

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, area);
}

fn render_stats(f: &mut Frame, app: &TuiApp, area: Rect) {
    let error_color = if app.error_count > 0 {
        Color::Red
    } else {
        Color::Green
    };
    let pause_status = if app.paused { " (PAUSED)" } else { "" };

    let lines = vec![
        Line::from(vec![
            Span::styled(
                "📊 Statistics",
                Style::default()
                    .fg(Color::Blue)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(pause_status, Style::default().fg(Color::Yellow)),
        ]),
        Line::from(vec![
            Span::styled("   Events: ", Style::default().fg(Color::Gray)),
            Span::styled(
                app.event_count.to_string(),
                Style::default().fg(Color::White),
            ),
            Span::styled("   Errors: ", Style::default().fg(Color::Gray)),
            Span::styled(
                app.error_count.to_string(),
                Style::default().fg(error_color),
            ),
        ]),
    ];

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, area);
}

fn render_recent_lifecycle_events(f: &mut Frame, app: &TuiApp, area: Rect) {
    if area.height < 2 {
        return;
    }

    let title_line = Line::from(vec![Span::styled(
        "⚡ Lifecycle Events",
        Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD),
    )]);

    let mut lines = vec![title_line];

    // Show the most recent lifecycle events
    let recent_events: Vec<&LifecycleEvent> = app
        .lifecycle_events
        .iter()
        .rev()
        .take((area.height - 1) as usize)
        .collect();

    for event in recent_events {
        let timestamp = event.timestamp.format("%H:%M:%S").to_string();
        let symbol = match event.event_type {
            LifecycleEventType::ActorStarted => "✅",
            LifecycleEventType::ActorStopped => "⏹️",
            LifecycleEventType::ActorError => "❌",
            LifecycleEventType::ActorResult => "📤",
            LifecycleEventType::StatusUpdate => "🔄",
        };

        lines.push(Line::from(vec![
            Span::styled(
                format!("   [{}] ", timestamp),
                Style::default().fg(Color::Gray),
            ),
            Span::styled(symbol, Style::default()),
            Span::styled(
                format!(" {}", event.message),
                Style::default().fg(Color::White),
            ),
        ]));
    }

    // Fill remaining space if no events
    if app.lifecycle_events.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "   No lifecycle events yet",
            Style::default().fg(Color::Gray),
        )]));
    }

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, area);
}

fn format_duration(start_time: chrono::DateTime<chrono::Utc>) -> String {
    let duration = chrono::Utc::now().signed_duration_since(start_time);
    let total_seconds = duration.num_seconds();

    if total_seconds < 60 {
        format!("{}s", total_seconds)
    } else if total_seconds < 3600 {
        let minutes = total_seconds / 60;
        let seconds = total_seconds % 60;
        format!("{}m {}s", minutes, seconds)
    } else {
        let hours = total_seconds / 3600;
        let minutes = (total_seconds % 3600) / 60;
        format!("{}h {}m", hours, minutes)
    }
}

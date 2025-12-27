use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
    Frame,
};

use super::super::app::EventExplorerApp;
use chrono::{TimeZone, Utc};

pub fn render_event_list(f: &mut Frame, app: &EventExplorerApp, area: Rect) {
    let title = format!(
        " Events ({}/{}) ",
        app.filtered_events.len(),
        app.events.len()
    );

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(if app.show_help {
            Style::default().fg(Color::Gray)
        } else {
            Style::default().fg(Color::Blue)
        });

    // Create list items from filtered events
    let items: Vec<ListItem> = app
        .filtered_events
        .iter()
        .enumerate()
        .map(|(i, &event_idx)| {
            let event = &app.events[event_idx];
            let is_selected = Some(i) == app.selected_event_index;
            create_event_list_item(event, is_selected)
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    f.render_stateful_widget(list, area, &mut app.list_state.clone());
}

fn create_event_list_item(event: &theater::chain::ChainEvent, is_selected: bool) -> ListItem<'_> {
    let timestamp = format_timestamp(event.timestamp);
    let event_type = &event.event_type;

    // Determine event level/color based on event type or description
    let (level_color, level_symbol) = categorize_event(event);

    let mut spans = vec![
        Span::styled(
            format!("[{}] ", timestamp),
            Style::default().fg(Color::Gray),
        ),
        Span::styled(level_symbol, Style::default().fg(level_color)),
        Span::styled(" ", Style::default()),
        Span::styled(
            event_type,
            if is_selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(level_color)
            },
        ),
    ];

    // Add description if it's different from event_type and not too long
    if let Some(description) = &event.description {
        if description != event_type && !description.is_empty() {
            let truncated = if description.len() > 40 {
                format!("{}...", &description[..37])
            } else {
                description.clone()
            };

            spans.push(Span::styled(
                format!(" - {}", truncated),
                Style::default().fg(if is_selected {
                    Color::White
                } else {
                    Color::Gray
                }),
            ));
        }
    }

    ListItem::new(Line::from(spans))
}

fn format_timestamp(timestamp: u64) -> String {
    let dt = Utc
        .timestamp_opt(timestamp as i64, 0)
        .single()
        .unwrap_or_else(|| Utc::now());
    dt.format("%H:%M:%S").to_string()
}

fn categorize_event(event: &theater::chain::ChainEvent) -> (Color, &'static str) {
    let event_type = &event.event_type;
    let description = event.description.as_deref().unwrap_or("");

    // Check for error indicators
    if event_type.contains("error") || description.to_lowercase().contains("error") {
        return (Color::Red, "❌");
    }

    // Check for warning indicators
    if event_type.contains("warn") || description.to_lowercase().contains("warn") {
        return (Color::Yellow, "⚠");
    }

    // Check for HTTP events
    if event_type.starts_with("http") {
        return (Color::Green, "🌐");
    }

    // Check for runtime events
    if event_type.starts_with("runtime") {
        return (Color::Cyan, "⚙");
    }

    // Check for WASM events
    if event_type.starts_with("wasm") {
        return (Color::Magenta, "📦");
    }

    // Default
    (Color::White, "ℹ")
}

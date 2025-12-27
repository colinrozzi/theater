use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

use crate::tui::app::{DisplayEvent, EventLevel, TuiApp};

pub fn render_event_panel(f: &mut Frame, app: &TuiApp, area: Rect) {
    let block = Block::default()
        .title(format!(" Events (Live) - {} total ", app.event_count))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));

    // Create list items from events
    let events: Vec<ListItem> = app
        .events
        .iter()
        .rev() // Show newest first
        .take(area.height.saturating_sub(2) as usize) // Account for borders
        .map(|event| create_event_list_item(event))
        .collect();

    let events_list = List::new(events)
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));

    f.render_widget(events_list, area);

    // Add auto-scroll indicator if enabled
    if app.auto_scroll {
        let scroll_indicator = Paragraph::new(" [Auto-scroll: ON] ")
            .style(Style::default().fg(Color::Green))
            .block(Block::default());

        let indicator_area = Rect {
            x: area.x + 2,
            y: area.y + area.height - 1,
            width: 17,
            height: 1,
        };

        f.render_widget(scroll_indicator, indicator_area);
    }
}

fn create_event_list_item(event: &DisplayEvent) -> ListItem<'_> {
    let timestamp = event.timestamp.format("%H:%M:%S").to_string();

    let (level_color, level_symbol) = match event.level {
        EventLevel::Info => (Color::White, " "),
        EventLevel::Warning => (Color::Yellow, "⚠"),
        EventLevel::Error => (Color::Red, "❌"),
    };

    let mut spans = vec![
        Span::styled(
            format!("[{}] ", timestamp),
            Style::default().fg(Color::Gray),
        ),
        Span::styled(level_symbol, Style::default().fg(level_color)),
        Span::styled(
            format!(" {}", event.event_type),
            Style::default().fg(level_color),
        ),
    ];

    // Add message if it's different from event_type
    if event.message != event.event_type {
        spans.push(Span::styled(
            format!(" - {}", event.message),
            Style::default().fg(Color::White),
        ));
    }

    // Add details if available (on next line with indentation)
    let mut lines = vec![Line::from(spans)];

    if let Some(details) = &event.details {
        // Truncate details if too long
        let truncated_details = if details.len() > 80 {
            format!("{}...", &details[..77])
        } else {
            details.clone()
        };

        lines.push(Line::from(vec![
            Span::styled("    └─ ", Style::default().fg(Color::Gray)),
            Span::styled(truncated_details, Style::default().fg(Color::Cyan)),
        ]));
    }

    ListItem::new(lines)
}

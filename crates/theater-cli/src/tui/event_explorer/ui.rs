use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph},
    Frame,
};

use super::app::EventExplorerApp;
use super::components::{render_event_list, render_event_detail, render_help_modal};

pub fn render_explorer_ui(f: &mut Frame, app: &EventExplorerApp) {
    let size = f.size();

    // Main layout: title + content + status
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Title bar
            Constraint::Min(10),   // Main content
            Constraint::Length(1), // Status bar
        ])
        .split(size);

    // Render title
    render_title_bar(f, app, main_chunks[0]);

    // Split main content: event list + details
    let content_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50), // Event list
            Constraint::Percentage(50), // Event details
        ])
        .split(main_chunks[1]);

    // Render main panels
    render_event_list(f, app, content_chunks[0]);
    render_event_detail(f, app, content_chunks[1]);

    // Render status bar
    render_status_bar(f, app, main_chunks[2]);

    // Render help modal if active
    if app.show_help {
        render_help_modal(f, app, size);
    }
}

fn render_title_bar(f: &mut Frame, app: &EventExplorerApp, area: Rect) {
    let actor_id_short = if app.actor_id.len() > 12 {
        format!("{}...", &app.actor_id[..9])
    } else {
        app.actor_id.clone()
    };

    let mut title_parts = vec![
        Span::styled(" Theater Event Explorer - Actor: ", Style::default().fg(Color::White)),
        Span::styled(actor_id_short, Style::default().fg(Color::Yellow)),
    ];

    // Add live mode indicator
    if app.live_mode {
        title_parts.push(Span::styled(" [LIVE", Style::default().fg(Color::Green)));
        if app.paused {
            title_parts.push(Span::styled(" - PAUSED", Style::default().fg(Color::Red)));
        }
        if app.follow_mode {
            title_parts.push(Span::styled(" - FOLLOW", Style::default().fg(Color::Cyan)));
        }
        title_parts.push(Span::styled("] ", Style::default().fg(Color::Green)));
    }

    let title_paragraph = Paragraph::new(Line::from(title_parts))
        .style(
            Style::default()
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        );

    f.render_widget(title_paragraph, area);
}

fn render_status_bar(f: &mut Frame, app: &EventExplorerApp, area: Rect) {
    let mut status_parts = vec![];

    // Event count info
    let event_info = format!(
        " Events: {}/{} ",
        app.filtered_events.len(),
        app.events.len()
    );
    status_parts.push(Span::styled(event_info, Style::default().fg(Color::White)));

    // Active filters
    if let Some(ref event_type) = app.active_filters.event_type {
        status_parts.push(Span::styled(
            format!("| Type: {} ", event_type),
            Style::default().fg(Color::Yellow),
        ));
    }

    if !app.search_query.is_empty() {
        status_parts.push(Span::styled(
            format!("| Search: {} ", app.search_query),
            Style::default().fg(Color::Cyan),
        ));
    }

    // Show scroll position if scrolled
    if app.detail_scroll_offset > 0 {
        status_parts.push(Span::styled(
            format!("| Scroll: {} ", app.detail_scroll_offset),
            Style::default().fg(Color::Magenta),
        ));
    }

    // Add spacer to push controls to the right
    let controls = " [q]uit [h]elp [Tab] cycle [←→] scroll ";
    let spacer_width = area
        .width
        .saturating_sub(controls.len() as u16)
        .saturating_sub(
            status_parts
                .iter()
                .map(|s| s.content.len() as u16)
                .sum::<u16>(),
        );

    if spacer_width > 0 {
        status_parts.push(Span::styled(
            " ".repeat(spacer_width as usize),
            Style::default(),
        ));
    }

    status_parts.push(Span::styled(controls, Style::default().fg(Color::Gray)));

    let status_paragraph = Paragraph::new(Line::from(status_parts))
        .style(Style::default().bg(Color::DarkGray));

    f.render_widget(status_paragraph, area);
}

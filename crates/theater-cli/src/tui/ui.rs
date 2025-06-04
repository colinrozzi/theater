use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::Paragraph,
    Frame,
};

use crate::tui::{
    app::TuiApp,
    components::{render_event_panel, render_status_panel},
};

pub fn render_ui(f: &mut Frame, app: &TuiApp) {
    let size = f.size();

    // Create the main layout: title bar + main content + controls
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Title bar
            Constraint::Min(10),   // Main content
            Constraint::Length(1), // Controls bar
        ])
        .split(size);

    // Render title bar
    render_title_bar(f, app, main_chunks[0]);

    // Split main content into two panels
    let content_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(60), // Events panel (left)
            Constraint::Percentage(40), // Status panel (right)
        ])
        .split(main_chunks[1]);

    // Render the two main panels
    render_event_panel(f, app, content_chunks[0]);
    render_status_panel(f, app, content_chunks[1]);

    // Render controls bar
    render_controls_bar(f, app, main_chunks[2]);
}

fn render_title_bar(f: &mut Frame, app: &TuiApp, area: Rect) {
    let actor_id_short = if app.actor_id.len() > 12 {
        format!("{}...", &app.actor_id[..9])
    } else {
        app.actor_id.clone()
    };

    let title = format!(" Theater Actor Monitor - Actor ID: {} ", actor_id_short);
    
    let title_paragraph = Paragraph::new(title)
        .style(Style::default()
            .fg(Color::White)
            .bg(Color::Blue)
            .add_modifier(Modifier::BOLD));

    f.render_widget(title_paragraph, area);
}

fn render_controls_bar(f: &mut Frame, app: &TuiApp, area: Rect) {
    let controls = if app.paused {
        " Controls: [q]uit [r]esume [s]croll [c]lear "
    } else {
        " Controls: [q]uit [p]ause [s]croll [c]lear "
    };

    let controls_paragraph = Paragraph::new(controls)
        .style(Style::default()
            .fg(Color::Black)
            .bg(Color::Gray));

    f.render_widget(controls_paragraph, area);
}

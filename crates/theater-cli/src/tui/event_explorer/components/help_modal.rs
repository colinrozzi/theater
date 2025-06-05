use ratatui::{
    layout::{Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use super::super::app::EventExplorerApp;
use super::super::input::HELP_TEXT;

pub fn render_help_modal(f: &mut Frame, _app: &EventExplorerApp, area: Rect) {
    // Calculate modal size (80% of screen, centered)
    let modal_width = (area.width as f32 * 0.8) as u16;
    let modal_height = (area.height as f32 * 0.8) as u16;
    
    let modal_x = (area.width.saturating_sub(modal_width)) / 2;
    let modal_y = (area.height.saturating_sub(modal_height)) / 2;
    
    let modal_area = Rect {
        x: modal_x,
        y: modal_y,
        width: modal_width,
        height: modal_height,
    };

    // Clear the background
    f.render_widget(Clear, modal_area);

    // Create the help content
    let help_lines: Vec<Line> = HELP_TEXT
        .lines()
        .map(|line| {
            if line.trim().is_empty() {
                Line::from("")
            } else if line.starts_with("Theater Event Explorer") || line.ends_with("Controls") {
                // Title lines
                Line::from(Span::styled(
                    line,
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ))
            } else if line.ends_with(":") {
                // Section headers
                Line::from(Span::styled(
                    line,
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ))
            } else if line.starts_with("  ") {
                // Key descriptions - parse key and description
                let trimmed = line.trim();
                if let Some(space_pos) = trimmed.find(' ') {
                    let (key_part, desc_part) = trimmed.split_at(space_pos);
                    Line::from(vec![
                        Span::styled("  ", Style::default()),
                        Span::styled(
                            key_part,
                            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(desc_part, Style::default().fg(Color::White)),
                    ])
                } else {
                    Line::from(Span::styled(line, Style::default().fg(Color::White)))
                }
            } else {
                // Regular lines
                Line::from(Span::styled(line, Style::default().fg(Color::White)))
            }
        })
        .collect();

    let help_block = Block::default()
        .title(" Help - Press h/Esc to close ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let help_paragraph = Paragraph::new(help_lines)
        .block(help_block)
        .wrap(Wrap { trim: true });

    f.render_widget(help_paragraph, modal_area);
}

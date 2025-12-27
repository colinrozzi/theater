use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
    Frame,
};

use super::super::app::{DetailMode, EventExplorerApp};
use chrono::{TimeZone, Utc};

pub fn render_event_detail(f: &mut Frame, app: &EventExplorerApp, area: Rect) {
    let title = format!(
        " Event Details - {} [Tab: {}] [←→ scroll] ",
        app.detail_mode.display_name(),
        app.detail_mode.next_mode_name()
    );

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(if app.show_help {
            Style::default().fg(Color::Gray)
        } else {
            Style::default().fg(Color::Green)
        });

    let content = if let Some(event) = app.get_selected_event() {
        match app.detail_mode {
            DetailMode::Overview => render_event_overview(event),
            DetailMode::JsonData => render_event_json(event),
            DetailMode::RawData => render_event_raw(event),
            DetailMode::ChainView => render_event_chain(event, app),
        }
    } else {
        vec![
            Line::from(""),
            Line::from(Span::styled(
                "No event selected",
                Style::default().fg(Color::Gray),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Use ↑↓ or j/k to navigate events",
                Style::default().fg(Color::Gray),
            )),
        ]
    };

    // Calculate available height for content (minus borders)
    let content_height = area.height.saturating_sub(2) as usize;
    let total_lines = content.len();

    // Clamp scroll offset to valid range
    let max_scroll = total_lines.saturating_sub(content_height);
    let scroll_offset = app.detail_scroll_offset.min(max_scroll);

    // Take the visible slice of content
    let visible_content: Vec<Line> = content
        .into_iter()
        .skip(scroll_offset)
        .take(content_height)
        .collect();

    let paragraph = Paragraph::new(visible_content)
        .block(block)
        .wrap(Wrap { trim: true });

    f.render_widget(paragraph, area);

    // Render scrollbar if content is longer than available space
    if total_lines > content_height {
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓"));

        let mut scrollbar_state = ScrollbarState::new(total_lines).position(scroll_offset);

        f.render_stateful_widget(
            scrollbar,
            area.inner(&ratatui::layout::Margin {
                vertical: 1,
                horizontal: 0,
            }),
            &mut scrollbar_state,
        );
    }
}

fn render_event_overview(event: &theater::chain::ChainEvent) -> Vec<Line<'_>> {
    let mut lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("Type: ", Style::default().fg(Color::Gray)),
            Span::styled(&event.event_type, Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::styled("Timestamp: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format_full_timestamp(event.timestamp),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(vec![
            Span::styled("Hash: ", Style::default().fg(Color::Gray)),
            Span::styled(format_hash(&event.hash), Style::default().fg(Color::Yellow)),
        ]),
    ];

    if let Some(parent_hash) = &event.parent_hash {
        lines.push(Line::from(vec![
            Span::styled("Parent: ", Style::default().fg(Color::Gray)),
            Span::styled(format_hash(parent_hash), Style::default().fg(Color::Yellow)),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::styled("Parent: ", Style::default().fg(Color::Gray)),
            Span::styled("None (root event)", Style::default().fg(Color::Green)),
        ]));
    }

    lines.push(Line::from(""));

    if let Some(description) = &event.description {
        lines.push(Line::from(vec![Span::styled(
            "Description:",
            Style::default().fg(Color::Gray),
        )]));
        lines.push(Line::from(""));

        // Split long descriptions into multiple lines
        let words: Vec<&str> = description.split_whitespace().collect();
        let mut current_line = String::new();

        for word in words {
            if current_line.len() + word.len() + 1 > 60 {
                if !current_line.is_empty() {
                    lines.push(Line::from(vec![
                        Span::styled("  ", Style::default()),
                        Span::styled(current_line.clone(), Style::default().fg(Color::White)),
                    ]));
                    current_line.clear();
                }
            }

            if !current_line.is_empty() {
                current_line.push(' ');
            }
            current_line.push_str(word);
        }

        if !current_line.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(current_line, Style::default().fg(Color::White)),
            ]));
        }
    } else {
        lines.push(Line::from(vec![
            Span::styled("Description: ", Style::default().fg(Color::Gray)),
            Span::styled("None", Style::default().fg(Color::Gray)),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Data Size: ", Style::default().fg(Color::Gray)),
        Span::styled(
            format_bytes(event.data.len()),
            Style::default().fg(Color::White),
        ),
    ]));

    // Data type and preview
    let data_type = detect_data_type(&event.data);
    lines.push(Line::from(vec![
        Span::styled("Data Type: ", Style::default().fg(Color::Gray)),
        Span::styled(data_type, Style::default().fg(Color::Cyan)),
    ]));

    // Add data preview in overview mode
    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        "Data Preview:",
        Style::default().fg(Color::Gray),
    )]));

    let preview_lines = render_data_preview(&event.data, 5); // Show up to 5 lines
    lines.extend(preview_lines);

    lines
}

fn render_event_json(event: &theater::chain::ChainEvent) -> Vec<Line<'_>> {
    let mut lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("Data Content (", Style::default().fg(Color::Gray)),
            Span::styled(
                format_bytes(event.data.len()),
                Style::default().fg(Color::White),
            ),
            Span::styled("):", Style::default().fg(Color::Gray)),
        ]),
        Line::from(""),
    ];

    // Try to parse event data as JSON first
    if let Ok(data_str) = std::str::from_utf8(&event.data) {
        if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(data_str) {
            if let Ok(pretty_json) = serde_json::to_string_pretty(&json_value) {
                lines.push(Line::from(vec![
                    Span::styled("Format: ", Style::default().fg(Color::Gray)),
                    Span::styled("JSON (pretty-printed)", Style::default().fg(Color::Green)),
                ]));
                lines.push(Line::from(""));

                let json_lines: Vec<&str> = pretty_json.lines().collect();
                for line in json_lines.iter() {
                    // Show all JSON lines
                    lines.push(Line::from(Span::styled(
                        line.to_string(),
                        Style::default().fg(Color::Cyan),
                    )));
                }

                return lines;
            }
        }
    }

    // Fallback to stringified bytes if not JSON
    lines.push(Line::from(vec![
        Span::styled("Format: ", Style::default().fg(Color::Gray)),
        Span::styled("Stringified bytes", Style::default().fg(Color::Yellow)),
    ]));
    lines.push(Line::from(""));

    let stringified = stringify_bytes(&event.data);
    let string_lines: Vec<&str> = stringified.lines().collect();

    for line in string_lines.iter() {
        // Show all stringified lines
        lines.push(Line::from(Span::styled(
            line.to_string(),
            Style::default().fg(Color::White),
        )));
    }

    lines
}

fn render_event_raw(event: &theater::chain::ChainEvent) -> Vec<Line<'_>> {
    let mut lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("Raw Data (", Style::default().fg(Color::Gray)),
            Span::styled(
                format_bytes(event.data.len()),
                Style::default().fg(Color::White),
            ),
            Span::styled("):", Style::default().fg(Color::Gray)),
        ]),
        Line::from(""),
    ];

    // Hex dump with ASCII - show all data with scrolling
    for (i, chunk) in event.data.chunks(16).enumerate() {
        let offset = format!("{:08x}", i * 16);
        let hex_part: Vec<String> = chunk.iter().map(|b| format!("{:02x}", b)).collect();
        let ascii_part: String = chunk
            .iter()
            .map(|&b| {
                if b.is_ascii_graphic() || b == b' ' {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();

        let hex_str = format!("{:<48}", hex_part.join(" "));

        lines.push(Line::from(vec![
            Span::styled(offset, Style::default().fg(Color::Yellow)),
            Span::styled("  ", Style::default()),
            Span::styled(hex_str, Style::default().fg(Color::Cyan)),
            Span::styled(" |", Style::default().fg(Color::Gray)),
            Span::styled(ascii_part, Style::default().fg(Color::White)),
            Span::styled("|", Style::default().fg(Color::Gray)),
        ]));
    }

    lines
}

fn render_event_chain<'a>(
    event: &'a theater::chain::ChainEvent,
    app: &'a EventExplorerApp,
) -> Vec<Line<'a>> {
    let mut lines = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            "Chain Context:",
            Style::default().fg(Color::Gray),
        )]),
        Line::from(""),
    ];

    // Find parent events
    if let Some(parent_hash) = &event.parent_hash {
        if let Some(parent) = find_event_by_hash(&app.events, parent_hash) {
            lines.push(Line::from(vec![
                Span::styled("Parent: ", Style::default().fg(Color::Gray)),
                Span::styled(&parent.event_type, Style::default().fg(Color::Blue)),
                Span::styled(" → ", Style::default().fg(Color::Gray)),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled("Parent: ", Style::default().fg(Color::Gray)),
                Span::styled("Missing (orphaned event)", Style::default().fg(Color::Red)),
            ]));
        }
    } else {
        lines.push(Line::from(vec![
            Span::styled("Parent: ", Style::default().fg(Color::Gray)),
            Span::styled("None (root event)", Style::default().fg(Color::Green)),
        ]));
    }

    // Current event
    lines.push(Line::from(vec![
        Span::styled("Current: ", Style::default().fg(Color::Gray)),
        Span::styled(&event.event_type, Style::default().fg(Color::Yellow)),
    ]));

    // Find child events
    let children = find_child_events(&app.events, &event.hash);
    if !children.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("Children: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("({} total)", children.len()),
                Style::default().fg(Color::Gray),
            ),
        ]));

        for child in children.iter().take(5) {
            // Show max 5 children
            lines.push(Line::from(vec![
                Span::styled("  → ", Style::default().fg(Color::Gray)),
                Span::styled(&child.event_type, Style::default().fg(Color::Green)),
            ]));
        }

        if children.len() > 5 {
            lines.push(Line::from(vec![
                Span::styled("  ... and ", Style::default().fg(Color::Gray)),
                Span::styled(
                    format!("{} more", children.len() - 5),
                    Style::default().fg(Color::Gray),
                ),
            ]));
        }
    } else {
        lines.push(Line::from(vec![
            Span::styled("Children: ", Style::default().fg(Color::Gray)),
            Span::styled("None (leaf event)", Style::default().fg(Color::Green)),
        ]));
    }

    lines
}

fn format_full_timestamp(timestamp: u64) -> String {
    let dt = Utc
        .timestamp_opt(timestamp as i64, 0)
        .single()
        .unwrap_or_else(|| Utc::now());
    dt.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

fn format_hash(hash: &[u8]) -> String {
    let hex = hex::encode(hash);
    if hex.len() > 16 {
        format!("{}...{}", &hex[..8], &hex[hex.len() - 8..])
    } else {
        hex
    }
}

fn format_bytes(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn detect_data_type(data: &[u8]) -> &'static str {
    if data.is_empty() {
        return "Empty";
    }

    if let Ok(text) = std::str::from_utf8(data) {
        if serde_json::from_str::<serde_json::Value>(text).is_ok() {
            "JSON"
        } else if text
            .chars()
            .all(|c| c.is_ascii_graphic() || c.is_ascii_whitespace())
        {
            "Text (UTF-8)"
        } else {
            "Text (UTF-8 with special chars)"
        }
    } else {
        "Binary"
    }
}

/// Stringify bytes into a readable format
fn stringify_bytes(data: &[u8]) -> String {
    if data.is_empty() {
        return "(empty)".to_string();
    }

    // Try UTF-8 first
    if let Ok(text) = std::str::from_utf8(data) {
        return text.to_string();
    }

    // If not valid UTF-8, convert each byte to its string representation
    // This will show the actual byte values in a readable way
    let mut result = String::new();
    for (i, &byte) in data.iter().enumerate() {
        if i > 0 && i % 16 == 0 {
            result.push('\n');
        } else if i > 0 && i % 8 == 0 {
            result.push_str("  ");
        } else if i > 0 {
            result.push(' ');
        }

        // Show printable ASCII as characters, others as hex
        if byte.is_ascii_graphic() || byte == b' ' {
            result.push_str(&format!("'{}'", byte as char));
        } else {
            result.push_str(&format!("0x{:02x}", byte));
        }
    }
    result
}

/// Render a preview of the data (first few lines)
fn render_data_preview(data: &[u8], max_lines: usize) -> Vec<Line<'_>> {
    let mut lines = Vec::new();

    if data.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled("(no data)", Style::default().fg(Color::Gray)),
        ]));
        return lines;
    }

    // Try UTF-8 first
    if let Ok(text) = std::str::from_utf8(data) {
        // Check if it's JSON for nice formatting
        if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(text) {
            if let Ok(pretty_json) = serde_json::to_string_pretty(&json_value) {
                let json_lines: Vec<&str> = pretty_json.lines().collect();
                for line in json_lines.iter().take(max_lines) {
                    lines.push(Line::from(vec![
                        Span::styled("  ", Style::default()),
                        Span::styled(line.to_string(), Style::default().fg(Color::Cyan)),
                    ]));
                }
                if json_lines.len() > max_lines {
                    lines.push(Line::from(vec![
                        Span::styled("  ", Style::default()),
                        Span::styled(
                            format!(
                                "... ({} more lines - switch to Data view)",
                                json_lines.len() - max_lines
                            ),
                            Style::default().fg(Color::Yellow),
                        ),
                    ]));
                }
                return lines;
            }
        }

        // Regular text
        for line in text.lines().take(max_lines) {
            let display_line = if line.len() > 60 {
                format!("{}...", &line[..57])
            } else {
                line.to_string()
            };
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(display_line, Style::default().fg(Color::White)),
            ]));
        }

        if text.lines().count() > max_lines {
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(
                    format!(
                        "... ({} more lines - switch to Data view)",
                        text.lines().count() - max_lines
                    ),
                    Style::default().fg(Color::Yellow),
                ),
            ]));
        }
    } else {
        // Binary data - show first few bytes
        let preview_bytes = &data[..data.len().min(32)];
        let mut preview = String::new();

        for (i, &byte) in preview_bytes.iter().enumerate() {
            if i > 0 {
                preview.push(' ');
            }
            if byte.is_ascii_graphic() {
                preview.push_str(&format!("'{}'", byte as char));
            } else {
                preview.push_str(&format!("0x{:02x}", byte));
            }
        }

        if data.len() > 32 {
            preview.push_str(" ...");
        }

        lines.push(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(preview, Style::default().fg(Color::Magenta)),
        ]));

        lines.push(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                "(binary data - use JSON or Raw view for details)",
                Style::default().fg(Color::Gray),
            ),
        ]));
    }

    lines
}

fn find_event_by_hash<'a>(
    events: &'a [theater::chain::ChainEvent],
    hash: &[u8],
) -> Option<&'a theater::chain::ChainEvent> {
    events.iter().find(|e| e.hash == hash)
}

fn find_child_events<'a>(
    events: &'a [theater::chain::ChainEvent],
    parent_hash: &[u8],
) -> Vec<&'a theater::chain::ChainEvent> {
    events
        .iter()
        .filter(|e| e.parent_hash.as_deref() == Some(parent_hash))
        .collect()
}

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use std::time::Duration;

use crate::tui::app::TuiApp;

pub enum InputEvent {
    Quit,
    TogglePause,
    ToggleAutoScroll,
    ClearEvents,
    None,
}

pub fn handle_input(app: &mut TuiApp) -> Result<InputEvent, Box<dyn std::error::Error>> {
    // Poll for events with a timeout
    if event::poll(Duration::from_millis(50))? {
        if let Event::Key(key) = event::read()? {
            // Only handle key press events, not key release
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Char('Q') => {
                        return Ok(InputEvent::Quit);
                    }
                    KeyCode::Char('p') | KeyCode::Char('P') => {
                        return Ok(InputEvent::TogglePause);
                    }
                    KeyCode::Char('r') | KeyCode::Char('R') => {
                        if app.paused {
                            return Ok(InputEvent::TogglePause);
                        }
                    }
                    KeyCode::Char('s') | KeyCode::Char('S') => {
                        return Ok(InputEvent::ToggleAutoScroll);
                    }
                    KeyCode::Char('c') | KeyCode::Char('C') => {
                        return Ok(InputEvent::ClearEvents);
                    }
                    KeyCode::Esc => {
                        return Ok(InputEvent::Quit);
                    }
                    _ => {}
                }
            }
        }
    }
    
    Ok(InputEvent::None)
}

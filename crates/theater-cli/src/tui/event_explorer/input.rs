use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crate::error::CliResult;

#[derive(Debug)]
pub enum ExplorerAction {
    Quit,
    NavigateUp,
    NavigateDown,
    PageUp,
    PageDown,
    ToggleDetailMode,
    ShowHelp,
    TogglePause,
    ToggleFollow,
    StartSearch,
    ClearSearch,
    StartFilter,
    ExportEvent,
    CopyToClipboard,
    None,
}

pub fn handle_input() -> CliResult<ExplorerAction> {
    if event::poll(std::time::Duration::from_millis(100)).map_err(|e| {
        crate::error::CliError::Internal(
            anyhow::anyhow!("Failed to poll for input: {}", e),
        )
    })? {
        if let Event::Key(key) = event::read().map_err(|e| crate::error::CliError::Internal(
            anyhow::anyhow!("Failed to read input: {}", e),
        ))? {
            return Ok(match key.code {
                KeyCode::Char('q') | KeyCode::Esc => ExplorerAction::Quit,
                KeyCode::Up | KeyCode::Char('k') => ExplorerAction::NavigateUp,
                KeyCode::Down | KeyCode::Char('j') => ExplorerAction::NavigateDown,
                KeyCode::PageUp => ExplorerAction::PageUp,
                KeyCode::PageDown => ExplorerAction::PageDown,
                KeyCode::Tab => ExplorerAction::ToggleDetailMode,
                KeyCode::Char('h') | KeyCode::F(1) => ExplorerAction::ShowHelp,
                KeyCode::Char('p') => ExplorerAction::TogglePause,
                KeyCode::Char(' ') => ExplorerAction::ToggleFollow,
                KeyCode::Char('/') => ExplorerAction::StartSearch,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    ExplorerAction::ClearSearch
                }
                KeyCode::Char('f') => ExplorerAction::StartFilter,
                KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    ExplorerAction::ExportEvent
                }
                KeyCode::Char('y') => ExplorerAction::CopyToClipboard,
                _ => ExplorerAction::None,
            });
        }
    }
    Ok(ExplorerAction::None)
}

pub const HELP_TEXT: &str = r#"
Theater Event Explorer - Keyboard Controls

Navigation:
  ↑/k          Move up in event list
  ↓/j          Move down in event list  
  Page Up/Down Page through events
  
Detail Views:
  Tab          Cycle through detail modes:
               Overview → JSON → Raw → Chain
  
Filtering & Search:
  f            Open filter dialog (future)
  /            Start search mode (future)
  Ctrl+C       Clear current search/filter
  
Live Mode:
  p            Pause/resume live updates
  Space        Toggle auto-scroll (follow mode)
  
Export & Clipboard:
  Ctrl+E       Export selected event (future)
  y            Copy event data to clipboard (future)
  
Other:
  h/?/F1       Show/hide this help
  q/Esc        Quit explorer

Phase 1 Implementation:
- Basic navigation and detail view cycling
- Help system
- Live mode controls (if --live)
"#;

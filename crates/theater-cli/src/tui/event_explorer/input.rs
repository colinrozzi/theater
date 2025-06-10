use crate::error::CliResult;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};

#[derive(Debug)]
pub enum ExplorerAction {
    Quit,
    NavigateUp,
    NavigateDown,
    PageUp,
    PageDown,
    ScrollDetailUp,
    ScrollDetailDown,
    ScrollDetailPageUp,
    ScrollDetailPageDown,
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
        crate::error::CliError::Internal(anyhow::anyhow!("Failed to poll for input: {}", e))
    })? {
        if let Event::Key(key) = event::read().map_err(|e| {
            crate::error::CliError::Internal(anyhow::anyhow!("Failed to read input: {}", e))
        })? {
            return Ok(match key.code {
                KeyCode::Char('q') | KeyCode::Esc => ExplorerAction::Quit,
                KeyCode::Up | KeyCode::Char('k') => ExplorerAction::NavigateUp,
                KeyCode::Down | KeyCode::Char('j') => ExplorerAction::NavigateDown,
                KeyCode::PageUp => ExplorerAction::PageUp,
                KeyCode::PageDown => ExplorerAction::PageDown,
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    ExplorerAction::ScrollDetailPageUp
                }
                KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    ExplorerAction::ScrollDetailPageDown
                }
                KeyCode::Left => ExplorerAction::ScrollDetailUp,
                KeyCode::Right => ExplorerAction::ScrollDetailDown,
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
  
Detail Panel Scrolling:
  ←/→          Scroll detail content up/down
  Ctrl+U/D     Page up/down in detail content
  
Detail Views:
  Tab          Cycle through detail modes:
               Overview → Data → Raw → Chain
               • Overview: Metadata + data preview
               • Data: Full stringified/JSON content  
               • Raw: Hex dump with ASCII
               • Chain: Parent/child relationships
  
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

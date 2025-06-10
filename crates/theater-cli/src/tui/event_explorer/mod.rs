pub mod app;
pub mod components;
pub mod input;
pub mod ui;

pub use app::EventExplorerApp;

use crate::{error::CliResult, CommandContext};
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::net::SocketAddr;
use tracing::debug;

pub async fn run_explorer(
    mut app: EventExplorerApp,
    ctx: &CommandContext,
    server_address: SocketAddr,
) -> CliResult<()> {
    debug!("Starting event explorer TUI");

    // Setup terminal
    enable_raw_mode().map_err(|e| {
        crate::error::CliError::Internal(anyhow::anyhow!("Failed to enable raw mode: {}", e))
    })?;

    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).map_err(|e| {
        crate::error::CliError::Internal(anyhow::anyhow!("Failed to enter alternate screen: {}", e))
    })?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(|e| {
        crate::error::CliError::Internal(anyhow::anyhow!("Failed to create terminal: {}", e))
    })?;

    // Hide cursor
    terminal.hide_cursor().map_err(|e| {
        crate::error::CliError::Internal(anyhow::anyhow!("Failed to hide cursor: {}", e))
    })?;

    // Main event loop
    let result = run_explorer_loop(&mut terminal, &mut app, ctx, server_address).await;

    // Cleanup terminal
    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
    terminal.show_cursor().ok();

    result
}

async fn run_explorer_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut EventExplorerApp,
    ctx: &CommandContext,
    server_address: SocketAddr,
) -> CliResult<()> {
    use input::{handle_input, ExplorerAction};

    debug!("Starting event explorer main loop");

    loop {
        // Render UI
        terminal
            .draw(|f| ui::render_explorer_ui(f, app))
            .map_err(|e| {
                crate::error::CliError::Internal(anyhow::anyhow!("Failed to draw terminal: {}", e))
            })?;

        // Handle input
        match handle_input()? {
            ExplorerAction::Quit => {
                debug!("User requested quit");
                break;
            }
            ExplorerAction::NavigateUp => {
                app.select_previous();
            }
            ExplorerAction::NavigateDown => {
                app.select_next();
            }
            ExplorerAction::PageUp => {
                app.page_up();
            }
            ExplorerAction::PageDown => {
                app.page_down();
            }
            ExplorerAction::ScrollDetailUp => {
                app.scroll_detail_up();
            }
            ExplorerAction::ScrollDetailDown => {
                app.scroll_detail_down();
            }
            ExplorerAction::ScrollDetailPageUp => {
                app.scroll_detail_page_up(10);
            }
            ExplorerAction::ScrollDetailPageDown => {
                app.scroll_detail_page_down(10);
            }
            ExplorerAction::ToggleDetailMode => {
                app.cycle_detail_mode();
            }
            ExplorerAction::ShowHelp => {
                app.toggle_help();
            }
            ExplorerAction::TogglePause => {
                app.toggle_pause();
            }
            ExplorerAction::ToggleFollow => {
                app.toggle_follow();
            }
            ExplorerAction::StartSearch => {
                app.enter_search_mode();
            }
            ExplorerAction::ClearSearch => {
                app.clear_search();
            }
            ExplorerAction::StartFilter => {
                app.enter_filter_mode();
            }
            ExplorerAction::None => {}
            _ => {
                // Handle other actions in future phases
                debug!("Unhandled action received");
            }
        }

        // Handle live updates if in live mode
        if app.live_mode && !app.paused {
            // TODO: In future phases, check for new events from Theater client
            // For now, this is a placeholder for live event streaming
        }

        if app.should_quit {
            debug!("App requested quit");
            break;
        }

        // Prevent excessive CPU usage
        tokio::time::sleep(tokio::time::Duration::from_millis(16)).await; // ~60 FPS
    }

    debug!("Event explorer loop ended");
    Ok(())
}

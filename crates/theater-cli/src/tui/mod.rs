pub mod app;
pub mod components;
pub mod events;
pub mod event_explorer;
pub mod ui;

use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    Terminal,
};
use std::io;
use tokio::sync::mpsc;
use tracing::debug;

use app::TuiApp;
use events::{handle_input, InputEvent};
use theater_server::ManagementResponse;
use ui::render_ui;

pub async fn run_tui(
    actor_id: String,
    manifest_path: String,
    mut response_rx: mpsc::UnboundedReceiver<ManagementResponse>,
) -> Result<(), Box<dyn std::error::Error>> {
    debug!("Starting TUI for actor: {}", actor_id);

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let mut app = TuiApp::new(actor_id, manifest_path);

    // Main TUI loop
    let result = run_tui_loop(&mut terminal, &mut app, &mut response_rx).await;

    // Cleanup terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

async fn run_tui_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut TuiApp,
    response_rx: &mut mpsc::UnboundedReceiver<ManagementResponse>,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        // Render the UI
        terminal.draw(|f| render_ui(f, app))?;

        // Handle input events
        match handle_input(app)? {
            InputEvent::Quit => {
                app.quit();
                break;
            }
            InputEvent::TogglePause => {
                app.toggle_pause();
                debug!("Toggled pause state: paused={}", app.paused);
            }
            InputEvent::ToggleAutoScroll => {
                app.toggle_auto_scroll();
                debug!("Toggled auto-scroll: enabled={}", app.auto_scroll);
            }
            InputEvent::ClearEvents => {
                app.reset_events();
                debug!("Cleared event history");
            }
            InputEvent::None => {}
        }

        // Handle incoming management responses (non-blocking)
        while let Ok(response) = response_rx.try_recv() {
            debug!("TUI received management response: {:?}", response);
            app.handle_management_response(response);
        }

        // Check if we should quit
        if app.should_quit {
            debug!("TUI quitting");
            break;
        }

        // Small delay to prevent excessive CPU usage
        tokio::time::sleep(tokio::time::Duration::from_millis(16)).await; // ~60 FPS
    }

    Ok(())
}

use std::io;

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use once_cell::sync::OnceCell;
use ratatui::{Terminal, backend::CrosstermBackend};
use tracing::{debug, error, info};
use tracing_subscriber::{EnvFilter, fmt};

use crate::{
    config::Config,
    session::SessionManager,
    ui::app::{self, App},
};

static LOGGING_INIT: OnceCell<()> = OnceCell::new();

pub fn init_logging(verbose: bool) -> anyhow::Result<()> {
    LOGGING_INIT
        .get_or_try_init(|| {
            let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                if verbose {
                    EnvFilter::new("bindr=debug,reqwest=info")
                } else {
                    EnvFilter::new("bindr=warn,reqwest=warn")
                }
            });

            let subscriber = fmt()
                .with_env_filter(env_filter)
                .with_target(true)
                .with_level(true)
                .with_file(verbose)
                .with_line_number(verbose)
                .with_ansi(false)
                .json()
                .finish();

            tracing::subscriber::set_global_default(subscriber).map_err(|e| anyhow::anyhow!(e))?;

            info!(is_verbose = verbose, "initialized structured logging");
            Ok(())
        })
        .map(|_| ())
}

pub async fn list_projects() -> anyhow::Result<()> {
    info!("listing projects from configuration");

    let config = Config::load()?;
    let mut session_manager = SessionManager::new(config);
    session_manager.load_sessions()?;

    let sessions = session_manager.list_sessions();

    info!(
        project_count = sessions.len(),
        "loaded sessions for listing"
    );

    if sessions.is_empty() {
        println!("📭 No projects yet. Run 'bindr' to start your first project!");
    } else {
        println!("📋 Your Bindr projects:\n");
        for session in sessions {
            println!(
                "  • {} (Mode: {})",
                session.project_name,
                session.current_mode.display_name()
            );
        }
    }

    Ok(())
}

pub async fn open_project(name: &str) -> anyhow::Result<()> {
    info!(project = name, "opening project");

    let config = Config::load()?;
    let mut session_manager = SessionManager::new(config);
    session_manager.load_sessions()?;

    match session_manager.open_project(name) {
        Ok(session_id) => {
            info!(project = name, session_id = %session_id, "project opened successfully");
            println!("📂 Opening project: {name}");
            println!("Session ID: {session_id}");
            println!(
                "💡 In future versions, this will launch the TUI for the project's current mode."
            );
        }
        Err(e) => {
            error!(project = name, error = %e, "failed to open project");
            println!("❌ Failed to open project '{name}': {e}");
        }
    }

    Ok(())
}

pub async fn run_tui() -> Result<(), io::Error> {
    info!("starting TUI runtime");

    let config = Config::load().map_err(|e| {
        error!(error = %e, "failed to load configuration");
        io::Error::other(e)
    })?;
    let mut session_manager = SessionManager::new(config.clone());
    session_manager.load_sessions().map_err(|e| {
        error!(error = %e, "failed to load sessions");
        io::Error::other(e)
    })?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(config, session_manager).await.map_err(|err| {
        error!(error = %err, "failed to initialize app state");
        io::Error::other(err)
    })?;
    let res = app::run_app(&mut terminal, &mut app).await;

    debug!("TUI event loop exited");

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        error!(error = %err, "TUI exited with error");
        println!("Error: {err:?}");
    }

    info!("TUI runtime shutdown complete");
    Ok(())
}

// src/main.rs
use clap::{Parser, Subcommand};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal,
};
use std::io;
use std::path::PathBuf;
use tokio::sync::mpsc;

mod events;
mod config;
mod session;

use events::AppEvent;
use config::Config;
use session::SessionManager;

// Dark mode color palette
const BG_PRIMARY: Color = Color::Rgb(16, 18, 24);      // Deep blue-black
const BG_SECONDARY: Color = Color::Rgb(24, 27, 36);    // Slightly lighter
const TEXT_PRIMARY: Color = Color::Rgb(220, 223, 228); // Light gray
const TEXT_SECONDARY: Color = Color::Rgb(140, 147, 165); // Muted gray
const ACCENT_BLUE: Color = Color::Rgb(88, 166, 255);   // Bright blue
const ACCENT_GREEN: Color = Color::Rgb(80, 250, 123);  // Neon green
const ACCENT_YELLOW: Color = Color::Rgb(241, 196, 15); // Warm yellow
const ACCENT_RED: Color = Color::Rgb(255, 85, 85);     // Soft red
const BORDER_COLOR: Color = Color::Rgb(48, 52, 70);    // Subtle border

#[derive(Parser)]
#[command(name = "bindr")]
#[command(version = "0.1.0")]
#[command(about = "Multi-agent LLM workflow orchestration", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// List all projects
    List,
    /// Open an existing project
    Open { name: String },
}

enum AppView {
    Home,
    AddKey,
    Brainstorm,
    Plan,
    Execute,
    Document,
}

struct App {
    view: AppView,
    key_input: String,
    config: Config,
    session_manager: SessionManager,
    app_event_tx: mpsc::UnboundedSender<AppEvent>,
    app_event_rx: mpsc::UnboundedReceiver<AppEvent>,
}

impl App {
    fn new(config: Config, session_manager: SessionManager) -> (Self, mpsc::UnboundedSender<AppEvent>) {
        let (app_event_tx, app_event_rx) = mpsc::unbounded_channel();
        
        let app = App {
            view: AppView::Home,
            key_input: String::new(),
            config,
            session_manager,
            app_event_tx: app_event_tx.clone(),
            app_event_rx,
        };
        
        (app, app_event_tx)
    }
    
    fn get_usage_info(&self) -> (u32, u32) {
        self.config.get_usage_info()
    }
}

fn get_bindr_projects_path() -> PathBuf {
    let home = dirs::home_dir().expect("Could not find home directory");
    home.join(".bindr").join("projects")
}

async fn list_projects() -> anyhow::Result<()> {
    let config = Config::load()?;
    let mut session_manager = SessionManager::new(config);
    session_manager.load_sessions()?;
    
    let sessions = session_manager.list_sessions();
    
    if sessions.is_empty() {
        println!("📭 No projects yet. Run 'bindr' to start your first project!");
    } else {
        println!("📋 Your Bindr projects:\n");
        for session in sessions {
            println!("  • {} (Mode: {})", session.project_name, session.current_mode.display_name());
        }
    }
    
    Ok(())
}

async fn open_project(name: &str) -> anyhow::Result<()> {
    let config = Config::load()?;
    let mut session_manager = SessionManager::new(config);
    session_manager.load_sessions()?;
    
    match session_manager.open_project(name) {
        Ok(session_id) => {
            println!("📂 Opening project: {}", name);
            println!("Session ID: {}", session_id);
            println!("💡 In future versions, this will launch the TUI for the project's current mode.");
        }
        Err(e) => {
            println!("❌ Failed to open project '{}': {}", name, e);
        }
    }
    
    Ok(())
}

async fn run_tui() -> Result<(), io::Error> {
    // Load configuration
    let config = Config::load().map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    let mut session_manager = SessionManager::new(config.clone());
    session_manager.load_sessions().map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let (mut app, _app_event_tx) = App::new(config, session_manager);
    let res = run_app(&mut terminal, &mut app).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("Error: {:?}", err);
    }

    Ok(())
}

fn draw_home_view<B: ratatui::backend::Backend>(f: &mut ratatui::Frame, app: &App, chunks: Vec<ratatui::layout::Rect>) {
    // Header with usage counter
    let header_text = vec![
        Line::from(vec![
            Span::styled("Bindr", Style::default().fg(ACCENT_BLUE).add_modifier(Modifier::BOLD)),
            Span::styled(" | ", Style::default().fg(TEXT_SECONDARY)),
            Span::styled(
                {
                    let (used, limit) = app.get_usage_info();
                    if app.config.has_api_key() {
                        format!("Unlimited Access")
                    } else {
                        format!("Free Tier ({}/{} messages today)", used, limit)
                    }
                },
                Style::default().fg(ACCENT_YELLOW)
            ),
        ]),
    ];
    
    let header = Paragraph::new(header_text)
        .style(Style::default().bg(BG_SECONDARY))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR))
        );
    f.render_widget(header, chunks[0]);

    // Main content
    let welcome_text = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Welcome to Bindr",
            Style::default().fg(ACCENT_BLUE).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "Multi-agent workflow orchestration",
            Style::default().fg(TEXT_SECONDARY).add_modifier(Modifier::ITALIC),
        )),
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled("What would you like to do?", Style::default().fg(TEXT_PRIMARY))),
        Line::from(""),
        Line::from(vec![
            Span::styled(" [N] ", Style::default().fg(BG_PRIMARY).bg(ACCENT_GREEN).add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::styled("Start new project", Style::default().fg(TEXT_PRIMARY)),
            Span::styled(" (brainstorm)", Style::default().fg(TEXT_SECONDARY)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(" [P] ", Style::default().fg(BG_PRIMARY).bg(ACCENT_BLUE).add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::styled("View all projects", Style::default().fg(TEXT_PRIMARY)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(" [K] ", Style::default().fg(BG_PRIMARY).bg(ACCENT_YELLOW).add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::styled("Add OpenRouter API key", Style::default().fg(TEXT_PRIMARY)),
            Span::styled(" (unlimited access)", Style::default().fg(ACCENT_GREEN)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(" [Q] ", Style::default().fg(BG_PRIMARY).bg(ACCENT_RED).add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::styled("Quit", Style::default().fg(TEXT_PRIMARY)),
        ]),
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(
            "💡 Tip: Add your API key for unlimited access to premium models",
            Style::default().fg(TEXT_SECONDARY).add_modifier(Modifier::ITALIC),
        )),
    ];

    let content = Paragraph::new(welcome_text)
        .style(Style::default().bg(BG_PRIMARY))
        .alignment(Alignment::Left)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR))
                .title(Span::styled(" Home ", Style::default().fg(ACCENT_BLUE)))
        );
    f.render_widget(content, chunks[1]);

    // Footer
    let footer_text = vec![
        if app.config.has_api_key() {
            Line::from(vec![
                Span::styled("Unlimited access enabled", Style::default().fg(ACCENT_GREEN)),
                Span::styled(" • Press ", Style::default().fg(TEXT_SECONDARY)),
                Span::styled("K", Style::default().fg(ACCENT_YELLOW).add_modifier(Modifier::BOLD)),
                Span::styled(" to manage API key", Style::default().fg(TEXT_SECONDARY)),
            ])
        } else {
            Line::from(vec![
                Span::styled("Using free tier with ", Style::default().fg(TEXT_SECONDARY)),
                Span::styled("limited models", Style::default().fg(ACCENT_YELLOW)),
                Span::styled(" • Press ", Style::default().fg(TEXT_SECONDARY)),
                Span::styled("K", Style::default().fg(ACCENT_GREEN).add_modifier(Modifier::BOLD)),
                Span::styled(" to unlock full access", Style::default().fg(TEXT_SECONDARY)),
            ])
        }
    ];
    
    let footer = Paragraph::new(footer_text)
        .style(Style::default().bg(BG_SECONDARY))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR))
        );
    f.render_widget(footer, chunks[2]);
}

fn draw_add_key_view<B: ratatui::backend::Backend>(f: &mut ratatui::Frame, app: &App, chunks: Vec<ratatui::layout::Rect>) {
    // Header
    let header = Paragraph::new("Bindr")
        .style(Style::default().fg(ACCENT_BLUE).bg(BG_SECONDARY).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR))
        );
    f.render_widget(header, chunks[0]);

    // Main content
    let key_text = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Add OpenRouter API Key",
            Style::default().fg(ACCENT_BLUE).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Get your API key from: https://openrouter.ai/keys",
            Style::default().fg(TEXT_SECONDARY),
        )),
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled("Your API Key:", Style::default().fg(TEXT_PRIMARY))),
        Line::from(""),
        Line::from(vec![
            Span::styled(" ", Style::default()),
            Span::styled(
                if app.key_input.is_empty() { 
                    "sk-or-v1-...".to_string() 
                } else { 
                    app.key_input.clone() 
                },
                Style::default()
                    .fg(if app.key_input.is_empty() { TEXT_SECONDARY } else { ACCENT_GREEN })
                    .bg(BG_SECONDARY)
            ),
            Span::styled(" _", Style::default().fg(ACCENT_BLUE)),
        ]),
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(
            "Press Enter to save • ESC to cancel",
            Style::default().fg(TEXT_SECONDARY).add_modifier(Modifier::ITALIC),
        )),
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled("Benefits:", Style::default().fg(ACCENT_GREEN).add_modifier(Modifier::BOLD))),
        Line::from(Span::styled("  ✓ Unlimited messages", Style::default().fg(TEXT_PRIMARY))),
        Line::from(Span::styled("  ✓ Access to premium models (GPT-4, Claude Opus)", Style::default().fg(TEXT_PRIMARY))),
        Line::from(Span::styled("  ✓ Faster response times", Style::default().fg(TEXT_PRIMARY))),
        Line::from(Span::styled("  ✓ Priority support", Style::default().fg(TEXT_PRIMARY))),
    ];

    let content = Paragraph::new(key_text)
        .style(Style::default().bg(BG_PRIMARY))
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: true })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR))
                .title(Span::styled(" API Key Setup ", Style::default().fg(ACCENT_YELLOW)))
        );
    f.render_widget(content, chunks[1]);

    // Footer
    let footer = Paragraph::new("Your API key is stored locally and never shared")
        .style(Style::default().fg(TEXT_SECONDARY).bg(BG_SECONDARY))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR))
        );
    f.render_widget(footer, chunks[2]);
}

fn draw_brainstorm_view<B: ratatui::backend::Backend>(f: &mut ratatui::Frame, _app: &App, chunks: Vec<ratatui::layout::Rect>) {
    let content = Paragraph::new("🧠 Brainstorm Mode - Coming Soon!")
        .style(Style::default().fg(ACCENT_BLUE).bg(BG_PRIMARY))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR))
                .title(Span::styled(" Brainstorm ", Style::default().fg(ACCENT_BLUE)))
        );
    f.render_widget(content, chunks[1]);
}

fn draw_plan_view<B: ratatui::backend::Backend>(f: &mut ratatui::Frame, _app: &App, chunks: Vec<ratatui::layout::Rect>) {
    let content = Paragraph::new("📋 Plan Mode - Coming Soon!")
        .style(Style::default().fg(ACCENT_GREEN).bg(BG_PRIMARY))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR))
                .title(Span::styled(" Plan ", Style::default().fg(ACCENT_GREEN)))
        );
    f.render_widget(content, chunks[1]);
}

fn draw_execute_view<B: ratatui::backend::Backend>(f: &mut ratatui::Frame, _app: &App, chunks: Vec<ratatui::layout::Rect>) {
    let content = Paragraph::new("⚡ Execute Mode - Coming Soon!")
        .style(Style::default().fg(ACCENT_YELLOW).bg(BG_PRIMARY))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR))
                .title(Span::styled(" Execute ", Style::default().fg(ACCENT_YELLOW)))
        );
    f.render_widget(content, chunks[1]);
}

fn draw_document_view<B: ratatui::backend::Backend>(f: &mut ratatui::Frame, _app: &App, chunks: Vec<ratatui::layout::Rect>) {
    let content = Paragraph::new("📝 Document Mode - Coming Soon!")
        .style(Style::default().fg(ACCENT_RED).bg(BG_PRIMARY))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR))
                .title(Span::styled(" Document ", Style::default().fg(ACCENT_RED)))
        );
    f.render_widget(content, chunks[1]);
}

async fn run_app<B: ratatui::backend::Backend>(terminal: &mut Terminal<B>, app: &mut App) -> io::Result<()> {
    loop {
        terminal.draw(|f| {
            let size = f.size();

            // Create layout
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(10),
                    Constraint::Length(3),
                ])
                .split(size);

            match app.view {
                AppView::Home => draw_home_view::<B>(f, app, chunks.to_vec()),
                AppView::AddKey => draw_add_key_view::<B>(f, app, chunks.to_vec()),
                AppView::Brainstorm => draw_brainstorm_view::<B>(f, app, chunks.to_vec()),
                AppView::Plan => draw_plan_view::<B>(f, app, chunks.to_vec()),
                AppView::Execute => draw_execute_view::<B>(f, app, chunks.to_vec()),
                AppView::Document => draw_document_view::<B>(f, app, chunks.to_vec()),
            }
        })?;

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match app.view {
                    AppView::Home => match key.code {
                        KeyCode::Char('q') | KeyCode::Char('Q') => return Ok(()),
                        KeyCode::Char('n') | KeyCode::Char('N') => {
                            app.view = AppView::Brainstorm;
                        }
                        KeyCode::Char('p') | KeyCode::Char('P') => {
                            // TODO: Show projects
                            return Ok(());
                        }
                        KeyCode::Char('k') | KeyCode::Char('K') => {
                            app.view = AppView::AddKey;
                        }
                        _ => {}
                    },
                    AppView::AddKey => match key.code {
                        KeyCode::Esc => {
                            app.view = AppView::Home;
                            app.key_input.clear();
                        }
                        KeyCode::Enter => {
                            if !app.key_input.is_empty() {
                                // TODO: Save API key
                                app.view = AppView::Home;
                                app.key_input.clear();
                            }
                        }
                        KeyCode::Char(c) => {
                            app.key_input.push(c);
                        }
                        KeyCode::Backspace => {
                            app.key_input.pop();
                        }
                        _ => {}
                    },
                    _ => {
                        // Handle other views
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Char('Q') => return Ok(()),
                            KeyCode::Esc => {
                                app.view = AppView::Home;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None => {
            if let Err(e) = run_tui().await {
                eprintln!("Error running TUI: {}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::List) => {
            list_projects().await?;
        }
        Some(Commands::Open { name }) => {
            open_project(&name).await?;
        }
    }
    
    Ok(())
}
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
use tokio::sync::mpsc;

mod events;
mod config;
mod session;
mod llm;
mod streaming;
mod agent;
mod ui;
mod prompts;
pub mod tools;


use events::{AppEvent, BindrMode};
use config::Config;
use session::SessionManager;
use agent::AgentManager;
use ui::conversation::ConversationManager;
use tools::ToolRequestOutcome;

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

struct AppState {
    should_quit: bool,
    show_model_selection: bool,
    current_mode: BindrMode,
    status_message: Option<String>,
    pending_tool: Option<ToolRequestOutcome>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            should_quit: false,
            show_model_selection: false,
            current_mode: BindrMode::Brainstorm,
            status_message: None,
            pending_tool: None,
        }
    }
}

#[derive(Subcommand)]
enum Commands {
    /// List all projects
    List,
    /// Open an existing project
    Open { name: String },
}

#[allow(dead_code)]
enum AppView {
    Home,
    SelectProvider,
    AddKey,
    SelectModel,
    CustomModelInput,
    Conversation,
    ModelSelection,
    Brainstorm,
    Plan,
    Execute,
    Document,
}

struct App {
    view: AppView,
    key_input: String,
    custom_model_input: String,
    config: Config,
    #[allow(dead_code)]
    agent_manager: AgentManager,
    conversation_manager: Option<ConversationManager>,
    #[allow(dead_code)]
    app_event_tx: mpsc::UnboundedSender<AppEvent>,
    #[allow(dead_code)]
    app_event_rx: mpsc::UnboundedReceiver<AppEvent>,
    conversation_lines: Vec<ratatui::text::Line<'static>>,
    is_streaming: bool,
    current_input: String,
    state: AppState,
    provider_selection: usize,
    model_selection: usize,
    model_switch_selection: usize,
}

impl App {
    fn new(config: Config, mut session_manager: SessionManager) -> (Self, mpsc::UnboundedSender<AppEvent>) {
        let (app_event_tx, app_event_rx) = mpsc::unbounded_channel();
        let agent_manager = AgentManager::new(config.clone(), session_manager.clone());

        let app = App {
            view: AppView::Home,
            key_input: String::new(),
            custom_model_input: String::new(),
            config,
            agent_manager,
            conversation_manager: None,
            app_event_tx: app_event_tx.clone(),
            app_event_rx,
            conversation_lines: Vec::new(),
            is_streaming: false,
            current_input: String::new(),
            state: AppState::default(),
            provider_selection: 0,
            model_selection: 0,
            model_switch_selection: 0,
        };

        (app, app_event_tx)
    }

    fn get_usage_info(&self) -> (u32, u32) {
        self.config.get_usage_info()
    }

    /// Start a new conversation
    fn start_new_conversation(&mut self) {
        if !self.config.has_api_key() {
            // No API key configured, go to provider selection
            self.view = AppView::SelectProvider;
            return;
        }

        // Create conversation manager
        let llm_client = crate::llm::LlmClient::new(self.config.clone());
        let mut conversation_manager = ConversationManager::new(
            self.agent_manager.clone(),
            llm_client,
            BindrMode::Brainstorm,
        );

        // Start the conversation
        conversation_manager.start_conversation();

        self.conversation_manager = Some(conversation_manager);
        self.view = AppView::Conversation;
    }

    fn sync_runtime_config(&mut self) {
        let config_clone = self.config.clone();
        self.agent_manager.update_config(config_clone.clone());
        if let Some(ref mut conversation_manager) = self.conversation_manager {
            conversation_manager.update_config(config_clone);
        }
    }
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
            Span::styled("Add API key", Style::default().fg(TEXT_PRIMARY)),
            //Span::styled(" (unlimited access)", Style::default().fg(ACCENT_GREEN)),
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
                Span::styled("API key configured", Style::default().fg(ACCENT_GREEN)),
                Span::styled(" • Press ", Style::default().fg(TEXT_SECONDARY)),
                Span::styled("K", Style::default().fg(ACCENT_YELLOW).add_modifier(Modifier::BOLD)),
                Span::styled(" to manage API keys", Style::default().fg(TEXT_SECONDARY)),
            ])
        } else {
            Line::from(vec![
                Span::styled("No API key configured", Style::default().fg(TEXT_SECONDARY)),
                Span::styled(" • Press ", Style::default().fg(TEXT_SECONDARY)),
                Span::styled("K", Style::default().fg(ACCENT_GREEN).add_modifier(Modifier::BOLD)),
                Span::styled(" to add API key", Style::default().fg(TEXT_SECONDARY)),
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

fn draw_select_provider_view<B: ratatui::backend::Backend>(f: &mut ratatui::Frame, app: &App, chunks: Vec<ratatui::layout::Rect>) {
    let providers = app.config.get_providers();
    let mut items = Vec::new();
    
    for (i, (id, provider)) in providers.iter().enumerate() {
        let style = if i == app.provider_selection {
            Style::default().fg(ACCENT_BLUE).bg(BG_SECONDARY)
        } else {
            Style::default().fg(TEXT_PRIMARY)
        };
        
        let has_key = app.config.api_keys.contains_key(*id) || 
            provider.api_key_env.as_ref()
                .map(|env| std::env::var(env).is_ok())
                .unwrap_or(false);
        
        let status = if has_key {
            "✓"
        } else {
            "○"
        };
        
        items.push(Line::from(vec![
            Span::styled(format!("{} ", status), Style::default().fg(if has_key { ACCENT_GREEN } else { TEXT_SECONDARY })),
            Span::styled(provider.name.clone(), style),
        ]));
    }
    
    let content = Paragraph::new(items)
        .style(Style::default().bg(BG_PRIMARY))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR))
                .title(Span::styled(" Select Provider ", Style::default().fg(ACCENT_BLUE)))
        );
    f.render_widget(content, chunks[1]);
    
    // Footer
    let footer_text = vec![
        Line::from(vec![
            Span::styled("↑↓", Style::default().fg(ACCENT_GREEN).add_modifier(Modifier::BOLD)),
            Span::styled(" navigate • ", Style::default().fg(TEXT_SECONDARY)),
            Span::styled("Enter", Style::default().fg(ACCENT_GREEN).add_modifier(Modifier::BOLD)),
            Span::styled(" select • ", Style::default().fg(TEXT_SECONDARY)),
            Span::styled("Esc", Style::default().fg(ACCENT_RED).add_modifier(Modifier::BOLD)),
            Span::styled(" back", Style::default().fg(TEXT_SECONDARY)),
        ]),
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
    let current_provider = app.config.get_current_provider();
    let provider_name = current_provider.map(|p| p.name.as_str()).unwrap_or("Unknown");
    
    let key_text = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("Add {} API Key", provider_name),
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
            "Press Enter to save and select model • ESC to cancel",
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

fn draw_select_model_view<B: ratatui::backend::Backend>(f: &mut ratatui::Frame, app: &App, chunks: Vec<ratatui::layout::Rect>) {
    let current_provider = app.config.get_current_provider();
    let mut items = Vec::new();
    
    if let Some(provider) = current_provider {
        for (i, model) in provider.models.iter().enumerate() {
            let style = if i == app.model_selection {
                Style::default().fg(ACCENT_BLUE).bg(BG_SECONDARY)
            } else {
                Style::default().fg(TEXT_PRIMARY)
            };
            
            let premium_indicator = if model.is_premium {
                "💎 "
            } else {
                "🆓 "
            };
            
            items.push(Line::from(vec![
                Span::styled(premium_indicator, Style::default().fg(if model.is_premium { ACCENT_YELLOW } else { ACCENT_GREEN })),
                Span::styled(model.name.clone(), style),
                Span::styled(format!(" - {}", model.description), Style::default().fg(TEXT_SECONDARY)),
            ]));
        }
    }
    
    let content = Paragraph::new(items)
        .style(Style::default().bg(BG_PRIMARY))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR))
                .title(Span::styled(" Select Model ", Style::default().fg(ACCENT_BLUE)))
        );
    f.render_widget(content, chunks[1]);
    
    // Footer
    let footer_text = vec![
        Line::from(vec![
            Span::styled("↑↓", Style::default().fg(ACCENT_GREEN).add_modifier(Modifier::BOLD)),
            Span::styled(" navigate • ", Style::default().fg(TEXT_SECONDARY)),
            Span::styled("Enter", Style::default().fg(ACCENT_GREEN).add_modifier(Modifier::BOLD)),
            Span::styled(" select • ", Style::default().fg(TEXT_SECONDARY)),
            Span::styled("Esc", Style::default().fg(ACCENT_RED).add_modifier(Modifier::BOLD)),
            Span::styled(" back", Style::default().fg(TEXT_SECONDARY)),
        ]),
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

fn draw_custom_model_input_view<B: ratatui::backend::Backend>(f: &mut ratatui::Frame, app: &App, chunks: Vec<ratatui::layout::Rect>) {
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
    let content_text = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Enter Custom OpenRouter Model Name",
            Style::default().fg(ACCENT_BLUE).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Examples:",
            Style::default().fg(TEXT_PRIMARY).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "  • meta-llama/llama-3.1-8b-instruct",
            Style::default().fg(TEXT_SECONDARY),
        )),
        Line::from(Span::styled(
            "  • microsoft/phi-3-medium-128k-instruct",
            Style::default().fg(TEXT_SECONDARY),
        )),
        Line::from(Span::styled(
            "  • google/gemini-1.5-flash",
            Style::default().fg(TEXT_SECONDARY),
        )),
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled("Model Name:", Style::default().fg(TEXT_PRIMARY))),
        Line::from(""),
        Line::from(vec![
            Span::styled(" ", Style::default()),
            Span::styled(
                if app.custom_model_input.is_empty() { 
                    "model-name-here".to_string() 
                } else { 
                    app.custom_model_input.clone() 
                },
                Style::default()
                    .fg(if app.custom_model_input.is_empty() { TEXT_SECONDARY } else { ACCENT_GREEN })
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
    ];

    let content = Paragraph::new(content_text)
        .style(Style::default().bg(BG_PRIMARY))
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: true })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR))
                .title(Span::styled(" Custom Model ", Style::default().fg(ACCENT_YELLOW)))
        );
    f.render_widget(content, chunks[1]);

    // Footer
    let footer = Paragraph::new("Enter any model name available on OpenRouter")
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

fn draw_model_selection_view<B: ratatui::backend::Backend>(f: &mut ratatui::Frame, app: &App, chunks: Vec<ratatui::layout::Rect>) {
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

    // Main content - show all models from all providers
    let providers = app.config.get_providers();
    let mut items = Vec::new();
    let mut current_index = 0;
    
    // Show current model first
    let current_provider = app.config.get_current_provider();
    let current_model = app.config.default_model.clone();
    
    if let Some(provider) = current_provider {
        if let Some(model) = provider.models.iter().find(|m| m.id == current_model) {
            let premium_indicator = if model.is_premium { "💎 " } else { "🆓 " };
            items.push(Line::from(vec![
                Span::styled("→ ", Style::default().fg(ACCENT_GREEN).add_modifier(Modifier::BOLD)),
                Span::styled(premium_indicator, Style::default().fg(if model.is_premium { ACCENT_YELLOW } else { ACCENT_GREEN })),
                Span::styled(format!("{} ({})", model.name, provider.name), Style::default().fg(ACCENT_BLUE).add_modifier(Modifier::BOLD)),
                Span::styled(" - CURRENT", Style::default().fg(ACCENT_GREEN).add_modifier(Modifier::BOLD)),
            ]));
        }
    }
    
    items.push(Line::from(""));
    items.push(Line::from(Span::styled("Available Models:", Style::default().fg(TEXT_PRIMARY).add_modifier(Modifier::BOLD))));
    items.push(Line::from(""));
    
    // Add all models from all providers
    for (provider_id, provider) in providers.iter() {
        for model in &provider.models {
            let style = if current_index == app.model_switch_selection {
                Style::default().fg(ACCENT_BLUE).bg(BG_SECONDARY)
            } else {
                Style::default().fg(TEXT_PRIMARY)
            };
            
            let premium_indicator = if model.is_premium { "💎 " } else { "🆓 " };
            let is_current = model.id == current_model;
            
            items.push(Line::from(vec![
                Span::styled(premium_indicator, Style::default().fg(if model.is_premium { ACCENT_YELLOW } else { ACCENT_GREEN })),
                Span::styled(model.name.clone(), style),
                Span::styled(format!(" ({})", provider.name), Style::default().fg(TEXT_SECONDARY)),
                if is_current {
                    Span::styled(" - CURRENT", Style::default().fg(ACCENT_GREEN).add_modifier(Modifier::BOLD))
                } else {
                    Span::raw("")
                },
            ]));
            current_index += 1;
        }
    }
    
    let content = Paragraph::new(items)
        .style(Style::default().bg(BG_PRIMARY))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR))
                .title(Span::styled(" Switch Model ", Style::default().fg(ACCENT_BLUE)))
        );
    f.render_widget(content, chunks[1]);
    
    // Footer
    let footer_text = vec![
        Line::from(vec![
            Span::styled("↑↓", Style::default().fg(ACCENT_GREEN).add_modifier(Modifier::BOLD)),
            Span::styled(" navigate • ", Style::default().fg(TEXT_SECONDARY)),
            Span::styled("Enter", Style::default().fg(ACCENT_GREEN).add_modifier(Modifier::BOLD)),
            Span::styled(" select • ", Style::default().fg(TEXT_SECONDARY)),
            Span::styled("Esc", Style::default().fg(ACCENT_RED).add_modifier(Modifier::BOLD)),
            Span::styled(" back to conversation", Style::default().fg(TEXT_SECONDARY)),
        ]),
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

fn draw_conversation_view<B: ratatui::backend::Backend>(f: &mut ratatui::Frame, app: &mut App, chunks: Vec<ratatui::layout::Rect>) {
    if let Some(ref mut conversation_manager) = app.conversation_manager {
        // Render conversation manager components individually
        conversation_manager.render_conversation_ui(chunks[1], f.buffer_mut());
    }
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
                AppView::SelectProvider => draw_select_provider_view::<B>(f, app, chunks.to_vec()),
                AppView::AddKey => draw_add_key_view::<B>(f, app, chunks.to_vec()),
                AppView::SelectModel => draw_select_model_view::<B>(f, app, chunks.to_vec()),
                AppView::CustomModelInput => draw_custom_model_input_view::<B>(f, app, chunks.to_vec()),
                AppView::Conversation => draw_conversation_view::<B>(f, app, chunks.to_vec()),
                AppView::ModelSelection => draw_model_selection_view::<B>(f, app, chunks.to_vec()),
                AppView::Brainstorm => draw_brainstorm_view::<B>(f, app, chunks.to_vec()),
                AppView::Plan => draw_plan_view::<B>(f, app, chunks.to_vec()),
                AppView::Execute => draw_execute_view::<B>(f, app, chunks.to_vec()),
                AppView::Document => draw_document_view::<B>(f, app, chunks.to_vec()),
            }
        })?;

        // Process streaming chunks for conversation
        if let Some(ref mut conversation_manager) = app.conversation_manager {
            conversation_manager.process_streaming_chunks();
        }

        // Handle keyboard input with a short timeout to keep the loop responsive
        if event::poll(std::time::Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match app.view {
                    AppView::Home => match key.code {
                        KeyCode::Char('q') | KeyCode::Char('Q') => return Ok(()),
                        KeyCode::Char('n') | KeyCode::Char('N') => {
                            app.start_new_conversation();
                        }
                        KeyCode::Char('p') | KeyCode::Char('P') => {
                            // TODO: Show projects
                            return Ok(());
                        }
                        KeyCode::Char('k') | KeyCode::Char('K') => {
                            app.view = AppView::SelectProvider;
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
                                let provider_id = app.config.selected_provider.clone();
                                app.config.set_api_key(provider_id, app.key_input.clone());
                                if let Err(e) = app.config.save() {
                                    eprintln!("Failed to save config: {}", e);
                                }

                                app.sync_runtime_config();

                                app.key_input.clear();
                                app.view = AppView::SelectModel;
                                if let Some(ref mut cm) = app.conversation_manager {
                                    cm.set_focus(false);
                                }
                            }
                        }
                        KeyCode::Char('m') | KeyCode::Char('M') => {
                            app.view = AppView::SelectModel;
                            if let Some(ref mut cm) = app.conversation_manager {
                                cm.set_focus(false);
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
                    AppView::SelectProvider => match key.code {
                        KeyCode::Up => {
                            if app.provider_selection > 0 {
                                app.provider_selection -= 1;
                            }
                        }
                        KeyCode::Down => {
                            let providers = app.config.get_providers();
                            if app.provider_selection < providers.len().saturating_sub(1) {
                                app.provider_selection += 1;
                            }
                        }
                        KeyCode::Enter => {
                            let providers = app.config.get_providers();
                            if let Some((provider_id, provider)) = providers.get(app.provider_selection) {
                                let provider_id_str = provider_id.to_string();

                                // Check if API key already exists for this provider
                                let has_api_key = app.config.api_keys.contains_key(*provider_id)
                                    || provider
                                        .api_key_env
                                        .as_ref()
                                        .map(|env| std::env::var(env).is_ok())
                                        .unwrap_or(false);

                                // Now we can safely mutate config
                                app.config.set_selected_provider(provider_id_str);
                                app.sync_runtime_config();

                                if has_api_key {
                                    // API key exists, go directly to model selection
                                    app.view = AppView::SelectModel;
                                } else {
                                    // No API key, go to add key
                                    app.view = AppView::AddKey;
                                }
                            }
                        }
                        KeyCode::Esc => {
                            app.view = AppView::Home;
                        }
                        _ => {}
                    },
                    AppView::SelectModel => match key.code {
                        KeyCode::Up => {
                            if app.model_selection > 0 {
                                app.model_selection -= 1;
                            }
                        }
                        KeyCode::Down => {
                            if let Some(provider) = app.config.get_current_provider() {
                                if app.model_selection < provider.models.len().saturating_sub(1) {
                                    app.model_selection += 1;
                                }
                            }
                        }
                        KeyCode::Enter => {
                            if let Some(provider) = app.config.get_current_provider() {
                                if let Some(model) = provider.models.get(app.model_selection) {
                                    if model.id == "custom-model" {
                                        app.view = AppView::CustomModelInput;
                                    } else {
                                        app.config.default_model = model.id.clone();

                                        // Save the config with the new model
                                        if let Err(e) = app.config.save() {
                                            eprintln!("Failed to save config: {}", e);
                                        }

                                        app.sync_runtime_config();

                                        app.view = AppView::Home;
                                    }
                                }
                            }
                        }
                        KeyCode::Esc => {
                            app.view = AppView::SelectProvider;
                        }
                        _ => {}
                    },
                    AppView::CustomModelInput => match key.code {
                        KeyCode::Esc => {
                            app.view = AppView::SelectModel;
                            app.custom_model_input.clear();
                        }
                        KeyCode::Enter => {
                            if !app.custom_model_input.is_empty() {
                                app.config.set_custom_model(app.custom_model_input.clone());

                                // Save the config with the custom model
                                if let Err(e) = app.config.save() {
                                    eprintln!("Failed to save config: {}", e);
                                }

                                app.sync_runtime_config();

                                app.view = AppView::Home;
                                app.custom_model_input.clear();
                            }
                        }
                        KeyCode::Char(c) => {
                            app.custom_model_input.push(c);
                        }
                        KeyCode::Backspace => {
                            app.custom_model_input.pop();
                        }
                        _ => {}
                    },
                    AppView::Conversation => {
                        if let Some(ref mut conversation_manager) = app.conversation_manager {
                            match conversation_manager.handle_key(key).await {
                                Ok(action) => match action {
                                    crate::ui::conversation::manager::ConversationAction::GoHome => {
                                        app.view = AppView::Home;
                                        app.conversation_manager = None;
                                    }
                                    crate::ui::conversation::manager::ConversationAction::Exit => {
                                        return Ok(());
                                    }
                                    crate::ui::conversation::manager::ConversationAction::ShowModelSelection => {
                                        if let Some(ref mut cm) = app.conversation_manager {
                                            cm.set_focus(false);
                                        }
                                        app.view = AppView::ModelSelection;
                                        app.model_switch_selection = 0;
                                    }
                                    crate::ui::conversation::manager::ConversationAction::None => {}
                                },
                                Err(e) => {
                                    eprintln!("Error handling input: {}", e);
                                }
                            }
                        }
                    },
                    AppView::ModelSelection => match key.code {
                        KeyCode::Up => {
                            if app.model_switch_selection > 0 {
                                app.model_switch_selection -= 1;
                            }
                        }
                        KeyCode::Down => {
                            let providers = app.config.get_providers();
                            let total_models: usize = providers
                                .iter()
                                .map(|(_, provider)| provider.models.len())
                                .sum();
                            if app.model_switch_selection < total_models.saturating_sub(1) {
                                app.model_switch_selection += 1;
                            }
                        }
                        KeyCode::Enter => {
                            // Find the selected model across all providers
                            let providers = app.config.get_providers();
                            let mut current_index = 0;
                            let mut selected_provider_id = None;
                            let mut selected_model_id = None;

                            for (provider_id, provider) in providers.iter() {
                                for model in &provider.models {
                                    if current_index == app.model_switch_selection {
                                        selected_provider_id = Some(provider_id.to_string());
                                        selected_model_id = Some(model.id.clone());
                                        break;
                                    }
                                    current_index += 1;
                                }
                                if selected_provider_id.is_some() {
                                    break;
                                }
                            }

                            if let (Some(provider_id), Some(model_id)) = (selected_provider_id, selected_model_id) {
                                // Switch to this provider and model
                                app.config.set_selected_provider(provider_id);
                                app.config.default_model = model_id;

                                // Save the config
                                if let Err(e) = app.config.save() {
                                    eprintln!("Failed to save config: {}", e);
                                }

                                app.sync_runtime_config();

                                // Return to conversation
                                app.view = AppView::Conversation;
                                if let Some(ref mut cm) = app.conversation_manager {
                                    cm.set_focus(true);
                                }
                            }
                        }
                        KeyCode::Esc => {
                            app.view = AppView::Conversation;
                            if let Some(ref mut cm) = app.conversation_manager {
                                cm.set_focus(true);
                            }
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
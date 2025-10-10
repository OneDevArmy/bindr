use std::io;

use crossterm::event::{self, Event, KeyCode};
use ratatui::{
    Terminal,
    backend::Backend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use tokio::sync::mpsc;

use crate::{
    agent::AgentManager,
    config::Config,
    events::{AppEvent, BindrMode},
    session::SessionManager,
    tools::ToolRequestOutcome,
    ui::conversation::{self, ConversationManager},
};

// Dark mode color palette
const BG_PRIMARY: Color = Color::Rgb(16, 18, 24); // Deep blue-black
const BG_SECONDARY: Color = Color::Rgb(24, 27, 36); // Slightly lighter
const TEXT_PRIMARY: Color = Color::Rgb(220, 223, 228); // Light gray
const TEXT_SECONDARY: Color = Color::Rgb(140, 147, 165); // Muted gray
const ACCENT_BLUE: Color = Color::Rgb(88, 166, 255); // Bright blue
const ACCENT_GREEN: Color = Color::Rgb(80, 250, 123); // Neon green
const ACCENT_YELLOW: Color = Color::Rgb(241, 196, 15); // Warm yellow
const ACCENT_RED: Color = Color::Rgb(255, 85, 85); // Soft red
const BORDER_COLOR: Color = Color::Rgb(48, 52, 70); // Subtle border

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

pub struct App {
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
    conversation_lines: Vec<Line<'static>>,
    is_streaming: bool,
    current_input: String,
    state: AppState,
    provider_selection: usize,
    model_selection: usize,
    model_switch_selection: usize,
}

impl App {
    pub fn new(
        config: Config,
        session_manager: SessionManager,
    ) -> (Self, mpsc::UnboundedSender<AppEvent>) {
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

    pub fn get_usage_info(&self) -> (u32, u32) {
        self.config.get_usage_info()
    }

    /// Start a new conversation
    pub fn start_new_conversation(&mut self) {
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

    pub fn sync_runtime_config(&mut self) {
        let config_clone = self.config.clone();
        self.agent_manager.update_config(config_clone.clone());
        if let Some(ref mut conversation_manager) = self.conversation_manager {
            conversation_manager.update_config(config_clone);
        }
    }
}

fn draw_home_view<B: Backend>(
    f: &mut ratatui::Frame,
    app: &App,
    chunks: Vec<ratatui::layout::Rect>,
) {
    let header_text = vec![Line::from(vec![
        Span::styled(
            "Bindr",
            Style::default()
                .fg(ACCENT_BLUE)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" | ", Style::default().fg(TEXT_SECONDARY)),
        Span::styled(
            {
                let (used, limit) = app.get_usage_info();
                if app.config.has_api_key() {
                    "Unlimited Access".to_string()
                } else {
                    format!("Free Tier ({}/{} messages today)", used, limit)
                }
            },
            Style::default().fg(ACCENT_YELLOW),
        ),
    ])];

    let header = Paragraph::new(header_text)
        .style(Style::default().bg(BG_SECONDARY))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR)),
        );
    f.render_widget(header, chunks[0]);

    let welcome_text = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Welcome to Bindr",
            Style::default()
                .fg(ACCENT_BLUE)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "Multi-agent workflow orchestration",
            Style::default()
                .fg(TEXT_SECONDARY)
                .add_modifier(Modifier::ITALIC),
        )),
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(
            "What would you like to do?",
            Style::default().fg(TEXT_PRIMARY),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                " [N] ",
                Style::default()
                    .fg(BG_PRIMARY)
                    .bg(ACCENT_GREEN)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled("Start new project", Style::default().fg(TEXT_PRIMARY)),
            Span::styled(" (brainstorm)", Style::default().fg(TEXT_SECONDARY)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                " [P] ",
                Style::default()
                    .fg(BG_PRIMARY)
                    .bg(ACCENT_BLUE)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled("View all projects", Style::default().fg(TEXT_PRIMARY)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                " [K] ",
                Style::default()
                    .fg(BG_PRIMARY)
                    .bg(ACCENT_YELLOW)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled("Add API key", Style::default().fg(TEXT_PRIMARY)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                " [Q] ",
                Style::default()
                    .fg(BG_PRIMARY)
                    .bg(ACCENT_RED)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled("Quit", Style::default().fg(TEXT_PRIMARY)),
        ]),
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(
            "💡 Tip: Add your API key for unlimited access to premium models",
            Style::default()
                .fg(TEXT_SECONDARY)
                .add_modifier(Modifier::ITALIC),
        )),
    ];

    let content = Paragraph::new(welcome_text)
        .style(Style::default().bg(BG_PRIMARY))
        .alignment(Alignment::Left)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR))
                .title(Span::styled(" Home ", Style::default().fg(ACCENT_BLUE))),
        );
    f.render_widget(content, chunks[1]);

    let footer_text = vec![if app.config.has_api_key() {
        Line::from(vec![
            Span::styled("API key configured", Style::default().fg(ACCENT_GREEN)),
            Span::styled(" • Press ", Style::default().fg(TEXT_SECONDARY)),
            Span::styled(
                "K",
                Style::default()
                    .fg(ACCENT_YELLOW)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to manage API keys", Style::default().fg(TEXT_SECONDARY)),
        ])
    } else {
        Line::from(vec![
            Span::styled("No API key configured", Style::default().fg(TEXT_SECONDARY)),
            Span::styled(" • Press ", Style::default().fg(TEXT_SECONDARY)),
            Span::styled(
                "K",
                Style::default()
                    .fg(ACCENT_GREEN)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to add API key", Style::default().fg(TEXT_SECONDARY)),
        ])
    }];

    let footer = Paragraph::new(footer_text)
        .style(Style::default().bg(BG_SECONDARY))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR)),
        );
    f.render_widget(footer, chunks[2]);
}

fn draw_select_provider_view<B: Backend>(
    f: &mut ratatui::Frame,
    app: &App,
    chunks: Vec<ratatui::layout::Rect>,
) {
    let providers = app.config.get_providers();
    let mut items = Vec::new();

    for (i, (id, provider)) in providers.iter().enumerate() {
        let style = if i == app.provider_selection {
            Style::default().fg(ACCENT_BLUE).bg(BG_SECONDARY)
        } else {
            Style::default().fg(TEXT_PRIMARY)
        };

        let has_key = app.config.api_keys.contains_key(*id)
            || provider
                .api_key_env
                .as_ref()
                .map(|env| std::env::var(env).is_ok())
                .unwrap_or(false);

        let status = if has_key { "✓" } else { "○" };

        items.push(Line::from(vec![
            Span::styled(
                format!("{} ", status),
                Style::default().fg(if has_key {
                    ACCENT_GREEN
                } else {
                    TEXT_SECONDARY
                }),
            ),
            Span::styled(provider.name.clone(), style),
        ]));
    }

    let content = Paragraph::new(items)
        .style(Style::default().bg(BG_PRIMARY))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR))
                .title(Span::styled(
                    " Select Provider ",
                    Style::default().fg(ACCENT_BLUE),
                )),
        );
    f.render_widget(content, chunks[1]);

    let footer_text = vec![Line::from(vec![
        Span::styled(
            "↑↓",
            Style::default()
                .fg(ACCENT_GREEN)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" navigate • ", Style::default().fg(TEXT_SECONDARY)),
        Span::styled(
            "Enter",
            Style::default()
                .fg(ACCENT_GREEN)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" select • ", Style::default().fg(TEXT_SECONDARY)),
        Span::styled(
            "Esc",
            Style::default().fg(ACCENT_RED).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" back", Style::default().fg(TEXT_SECONDARY)),
    ])];

    let footer = Paragraph::new(footer_text)
        .style(Style::default().bg(BG_SECONDARY))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR)),
        );
    f.render_widget(footer, chunks[2]);
}

fn draw_add_key_view<B: Backend>(
    f: &mut ratatui::Frame,
    app: &App,
    chunks: Vec<ratatui::layout::Rect>,
) {
    let header = Paragraph::new("Bindr")
        .style(
            Style::default()
                .fg(ACCENT_BLUE)
                .bg(BG_SECONDARY)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR)),
        );
    f.render_widget(header, chunks[0]);

    let providers = app.config.get_providers();
    let current_provider = providers.get(app.provider_selection).map(|(_, p)| p);

    let content_text = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Add API Key",
            Style::default()
                .fg(ACCENT_BLUE)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Enter your API key below. It will be stored securely in your config file.",
            Style::default().fg(TEXT_SECONDARY),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "API Key:",
            Style::default()
                .fg(TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(" ", Style::default()),
            Span::styled(
                if app.key_input.is_empty() {
                    "Enter API key".to_string()
                } else {
                    "*".repeat(app.key_input.len())
                },
                Style::default()
                    .fg(if app.key_input.is_empty() {
                        TEXT_SECONDARY
                    } else {
                        ACCENT_GREEN
                    })
                    .bg(BG_SECONDARY),
            ),
            Span::styled(" _", Style::default().fg(ACCENT_BLUE)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Press Enter to save • ESC to cancel",
            Style::default()
                .fg(TEXT_SECONDARY)
                .add_modifier(Modifier::ITALIC),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Need to choose a model? Press M to browse available models.",
            Style::default().fg(TEXT_SECONDARY),
        )),
    ];

    let mut provider_info = Vec::new();
    if let Some(provider) = current_provider {
        provider_info.push(Line::from(""));
        provider_info.push(Line::from(Span::styled(
            format!("Provider: {}", provider.name),
            Style::default()
                .fg(ACCENT_BLUE)
                .add_modifier(Modifier::BOLD),
        )));
        provider_info.push(Line::from(Span::styled(
            "Premium models available",
            Style::default().fg(TEXT_SECONDARY),
        )));
    }

    let mut content_lines = content_text;
    content_lines.extend(provider_info);

    let content = Paragraph::new(content_lines)
        .style(Style::default().bg(BG_PRIMARY))
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: true })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR))
                .title(Span::styled(
                    " Add API Key ",
                    Style::default().fg(ACCENT_BLUE),
                )),
        );
    f.render_widget(content, chunks[1]);

    let footer = Paragraph::new("Enter your API key to unlock premium models")
        .style(Style::default().fg(TEXT_SECONDARY).bg(BG_SECONDARY))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR)),
        );
    f.render_widget(footer, chunks[2]);
}

fn draw_select_model_view<B: Backend>(
    f: &mut ratatui::Frame,
    app: &App,
    chunks: Vec<ratatui::layout::Rect>,
) {
    let header = Paragraph::new("Bindr")
        .style(
            Style::default()
                .fg(ACCENT_BLUE)
                .bg(BG_SECONDARY)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR)),
        );
    f.render_widget(header, chunks[0]);

    let providers = app.config.get_current_provider();
    let mut items = Vec::new();

    if let Some(provider) = providers {
        for (i, model) in provider.models.iter().enumerate() {
            let style = if i == app.model_selection {
                Style::default().fg(ACCENT_BLUE).bg(BG_SECONDARY)
            } else {
                Style::default().fg(TEXT_PRIMARY)
            };

            let premium_indicator = if model.is_premium { "💎 " } else { "🆓 " };

            items.push(Line::from(vec![
                Span::styled(
                    premium_indicator,
                    Style::default().fg(if model.is_premium {
                        ACCENT_YELLOW
                    } else {
                        ACCENT_GREEN
                    }),
                ),
                Span::styled(model.name.clone(), style),
                Span::styled(
                    format!(" - {}", model.description),
                    Style::default().fg(TEXT_SECONDARY),
                ),
            ]));
        }
    }

    let content = Paragraph::new(items)
        .style(Style::default().bg(BG_PRIMARY))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR))
                .title(Span::styled(
                    " Select Model ",
                    Style::default().fg(ACCENT_BLUE),
                )),
        );
    f.render_widget(content, chunks[1]);

    let footer_text = vec![Line::from(vec![
        Span::styled(
            "↑↓",
            Style::default()
                .fg(ACCENT_GREEN)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" navigate • ", Style::default().fg(TEXT_SECONDARY)),
        Span::styled(
            "Enter",
            Style::default()
                .fg(ACCENT_GREEN)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" select • ", Style::default().fg(TEXT_SECONDARY)),
        Span::styled(
            "Esc",
            Style::default().fg(ACCENT_RED).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" back", Style::default().fg(TEXT_SECONDARY)),
    ])];

    let footer = Paragraph::new(footer_text)
        .style(Style::default().bg(BG_SECONDARY))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR)),
        );
    f.render_widget(footer, chunks[2]);
}

fn draw_custom_model_input_view<B: Backend>(
    f: &mut ratatui::Frame,
    app: &App,
    chunks: Vec<ratatui::layout::Rect>,
) {
    let header = Paragraph::new("Bindr")
        .style(
            Style::default()
                .fg(ACCENT_BLUE)
                .bg(BG_SECONDARY)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR)),
        );
    f.render_widget(header, chunks[0]);

    let content_text = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Enter Custom OpenRouter Model Name",
            Style::default()
                .fg(ACCENT_BLUE)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Examples:",
            Style::default()
                .fg(TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
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
        Line::from(Span::styled(
            "Model Name:",
            Style::default().fg(TEXT_PRIMARY),
        )),
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
                    .fg(if app.custom_model_input.is_empty() {
                        TEXT_SECONDARY
                    } else {
                        ACCENT_GREEN
                    })
                    .bg(BG_SECONDARY),
            ),
            Span::styled(" _", Style::default().fg(ACCENT_BLUE)),
        ]),
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(
            "Press Enter to save • ESC to cancel",
            Style::default()
                .fg(TEXT_SECONDARY)
                .add_modifier(Modifier::ITALIC),
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
                .title(Span::styled(
                    " Custom Model ",
                    Style::default().fg(ACCENT_YELLOW),
                )),
        );
    f.render_widget(content, chunks[1]);

    let footer = Paragraph::new("Enter any model name available on OpenRouter")
        .style(Style::default().fg(TEXT_SECONDARY).bg(BG_SECONDARY))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR)),
        );
    f.render_widget(footer, chunks[2]);
}

fn draw_brainstorm_view<B: Backend>(
    f: &mut ratatui::Frame,
    _app: &App,
    chunks: Vec<ratatui::layout::Rect>,
) {
    let content = Paragraph::new("🧠 Brainstorm Mode - Coming Soon!")
        .style(Style::default().fg(ACCENT_BLUE).bg(BG_PRIMARY))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR))
                .title(Span::styled(
                    " Brainstorm ",
                    Style::default().fg(ACCENT_BLUE),
                )),
        );
    f.render_widget(content, chunks[1]);
}

fn draw_plan_view<B: Backend>(
    f: &mut ratatui::Frame,
    _app: &App,
    chunks: Vec<ratatui::layout::Rect>,
) {
    let content = Paragraph::new("📋 Plan Mode - Coming Soon!")
        .style(Style::default().fg(ACCENT_GREEN).bg(BG_PRIMARY))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR))
                .title(Span::styled(" Plan ", Style::default().fg(ACCENT_GREEN))),
        );
    f.render_widget(content, chunks[1]);
}

fn draw_execute_view<B: Backend>(
    f: &mut ratatui::Frame,
    _app: &App,
    chunks: Vec<ratatui::layout::Rect>,
) {
    let content = Paragraph::new("⚡ Execute Mode - Coming Soon!")
        .style(Style::default().fg(ACCENT_YELLOW).bg(BG_PRIMARY))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR))
                .title(Span::styled(
                    " Execute ",
                    Style::default().fg(ACCENT_YELLOW),
                )),
        );
    f.render_widget(content, chunks[1]);
}

fn draw_document_view<B: Backend>(
    f: &mut ratatui::Frame,
    _app: &App,
    chunks: Vec<ratatui::layout::Rect>,
) {
    let content = Paragraph::new("📝 Document Mode - Coming Soon!")
        .style(Style::default().fg(ACCENT_RED).bg(BG_PRIMARY))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR))
                .title(Span::styled(" Document ", Style::default().fg(ACCENT_RED))),
        );
    f.render_widget(content, chunks[1]);
}

fn draw_model_selection_view<B: Backend>(
    f: &mut ratatui::Frame,
    app: &App,
    chunks: Vec<ratatui::layout::Rect>,
) {
    let header = Paragraph::new("Bindr")
        .style(
            Style::default()
                .fg(ACCENT_BLUE)
                .bg(BG_SECONDARY)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR)),
        );
    f.render_widget(header, chunks[0]);

    let providers = app.config.get_providers();
    let mut items = Vec::new();
    let mut current_index = 0;

    let current_provider = app.config.get_current_provider();
    let current_model = app.config.default_model.clone();

    if let Some(provider) = current_provider {
        if let Some(model) = provider.models.iter().find(|m| m.id == current_model) {
            let premium_indicator = if model.is_premium { "💎 " } else { "🆓 " };
            items.push(Line::from(vec![
                Span::styled(
                    "→ ",
                    Style::default()
                        .fg(ACCENT_GREEN)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    premium_indicator,
                    Style::default().fg(if model.is_premium {
                        ACCENT_YELLOW
                    } else {
                        ACCENT_GREEN
                    }),
                ),
                Span::styled(
                    format!("{} ({})", model.name, provider.name),
                    Style::default()
                        .fg(ACCENT_BLUE)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    " - CURRENT",
                    Style::default()
                        .fg(ACCENT_GREEN)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }
    }

    items.push(Line::from(""));
    items.push(Line::from(Span::styled(
        "Available Models:",
        Style::default()
            .fg(TEXT_PRIMARY)
            .add_modifier(Modifier::BOLD),
    )));
    items.push(Line::from(""));

    for (provider_id, provider) in providers.iter() {
        for model in &provider.models {
            let style = if current_index == app.model_switch_selection {
                Style::default().fg(ACCENT_BLUE).bg(BG_SECONDARY)
            } else {
                Style::default().fg(TEXT_PRIMARY)
            };

            let premium_indicator = if model.is_premium { "💎 " } else { "🆓 " };

            items.push(Line::from(vec![
                Span::styled(
                    premium_indicator,
                    Style::default().fg(if model.is_premium {
                        ACCENT_YELLOW
                    } else {
                        ACCENT_GREEN
                    }),
                ),
                Span::styled(format!("{} ({})", model.name, provider.name), style),
                Span::styled(
                    format!(" - Provider ID: {}", provider_id),
                    Style::default().fg(TEXT_SECONDARY),
                ),
            ]));

            current_index += 1;
        }
    }

    let content = Paragraph::new(items)
        .style(Style::default().bg(BG_PRIMARY))
        .wrap(Wrap { trim: true })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR))
                .title(Span::styled(
                    " Switch Model ",
                    Style::default().fg(ACCENT_BLUE),
                )),
        );
    f.render_widget(content, chunks[1]);

    let footer_text = vec![Line::from(vec![
        Span::styled(
            "↑↓",
            Style::default()
                .fg(ACCENT_GREEN)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" navigate • ", Style::default().fg(TEXT_SECONDARY)),
        Span::styled(
            "Enter",
            Style::default()
                .fg(ACCENT_GREEN)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" select • ", Style::default().fg(TEXT_SECONDARY)),
        Span::styled(
            "Esc",
            Style::default().fg(ACCENT_RED).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" back", Style::default().fg(TEXT_SECONDARY)),
    ])];

    let footer = Paragraph::new(footer_text)
        .style(Style::default().bg(BG_SECONDARY))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_COLOR)),
        );
    f.render_widget(footer, chunks[2]);
}

fn draw_conversation_view<B: Backend>(
    f: &mut ratatui::Frame,
    app: &mut App,
    chunks: Vec<ratatui::layout::Rect>,
) {
    if let Some(ref mut conversation_manager) = app.conversation_manager {
        conversation_manager.render_conversation_ui(chunks[1], f.buffer_mut());
    }
}

pub async fn run_app<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> io::Result<()> {
    loop {
        terminal.draw(|f| {
            let size = f.size();

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
                AppView::CustomModelInput => {
                    draw_custom_model_input_view::<B>(f, app, chunks.to_vec())
                }
                AppView::Conversation => draw_conversation_view::<B>(f, app, chunks.to_vec()),
                AppView::ModelSelection => draw_model_selection_view::<B>(f, app, chunks.to_vec()),
                AppView::Brainstorm => draw_brainstorm_view::<B>(f, app, chunks.to_vec()),
                AppView::Plan => draw_plan_view::<B>(f, app, chunks.to_vec()),
                AppView::Execute => draw_execute_view::<B>(f, app, chunks.to_vec()),
                AppView::Document => draw_document_view::<B>(f, app, chunks.to_vec()),
            }
        })?;

        if let Some(ref mut conversation_manager) = app.conversation_manager {
            conversation_manager.process_streaming_chunks();
        }

        if event::poll(std::time::Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match app.view {
                    AppView::Home => match key.code {
                        KeyCode::Char('q') | KeyCode::Char('Q') => return Ok(()),
                        KeyCode::Char('n') | KeyCode::Char('N') => {
                            app.start_new_conversation();
                        }
                        KeyCode::Char('p') | KeyCode::Char('P') => {
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
                            if let Some((provider_id, provider)) =
                                providers.get(app.provider_selection)
                            {
                                let provider_id_str = provider_id.to_string();

                                let has_api_key = app.config.api_keys.contains_key(*provider_id)
                                    || provider
                                        .api_key_env
                                        .as_ref()
                                        .map(|env| std::env::var(env).is_ok())
                                        .unwrap_or(false);

                                app.config.set_selected_provider(provider_id_str);
                                app.sync_runtime_config();

                                if has_api_key {
                                    app.view = AppView::SelectModel;
                                } else {
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
                                    conversation::manager::ConversationAction::GoHome => {
                                        app.view = AppView::Home;
                                        app.conversation_manager = None;
                                    }
                                    conversation::manager::ConversationAction::Exit => {
                                        return Ok(());
                                    }
                                    conversation::manager::ConversationAction::ShowModelSelection => {
                                        if let Some(ref mut cm) = app.conversation_manager {
                                            cm.set_focus(false);
                                        }
                                        app.view = AppView::ModelSelection;
                                        app.model_switch_selection = 0;
                                    }
                                    conversation::manager::ConversationAction::None => {}
                                },
                                Err(e) => {
                                    eprintln!("Error handling input: {}", e);
                                }
                            }
                        }
                    }
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

                            if let (Some(provider_id), Some(model_id)) =
                                (selected_provider_id, selected_model_id)
                            {
                                app.config.set_selected_provider(provider_id);
                                app.config.default_model = model_id;

                                if let Err(e) = app.config.save() {
                                    eprintln!("Failed to save config: {}", e);
                                }

                                app.sync_runtime_config();

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
                    _ => match key.code {
                        KeyCode::Char('q') | KeyCode::Char('Q') => return Ok(()),
                        KeyCode::Esc => {
                            app.view = AppView::Home;
                        }
                        _ => {}
                    },
                }
            }
        }
    }
}

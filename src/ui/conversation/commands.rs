use strum::{IntoEnumIterator, AsRefStr, EnumIter, EnumString, IntoStaticStr};

/// Commands that can be invoked by starting a message with a leading slash.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString, EnumIter, AsRefStr, IntoStaticStr,
)]
#[strum(serialize_all = "kebab-case")]
pub enum SlashCommand {
    /// Switch to a different mode (brainstorm, plan, execute, document)
    Mode,
    /// Switch to a different model
    Model,
    /// Return to home screen
    Home,
    /// Exit the application
    Bye,
    /// Show help
    Help,
}

impl SlashCommand {
    /// User-visible description shown in help.
    pub fn description(self) -> &'static str {
        match self {
            SlashCommand::Mode => "switch to a different mode (brainstorm, plan, execute, document)",
            SlashCommand::Model => "switch to a different model",
            SlashCommand::Home => "return to the home screen",
            SlashCommand::Bye => "exit the application",
            SlashCommand::Help => "show available commands",
        }
    }

    /// Command string without the leading '/'.
    pub fn command(self) -> &'static str {
        self.into()
    }

    /// Whether this command can be run while streaming is active.
    pub fn available_during_streaming(self) -> bool {
        match self {
            SlashCommand::Mode | SlashCommand::Model | SlashCommand::Home | SlashCommand::Bye | SlashCommand::Help => true,
        }
    }
}

/// Return all built-in commands in a Vec paired with their command string.
pub fn built_in_slash_commands() -> Vec<(&'static str, SlashCommand)> {
    SlashCommand::iter()
        .map(|c| (c.command(), c))
        .collect()
}

/// Parse a slash command from user input
pub fn parse_slash_command(input: &str) -> Option<SlashCommand> {
    if !input.starts_with('/') {
        return None;
    }

    let command_str = &input[1..]; // Remove the leading '/'
    
    // Try to parse as enum
    if let Ok(command) = command_str.parse::<SlashCommand>() {
        return Some(command);
    }

    // Handle aliases
    match command_str.to_lowercase().as_str() {
        "q" | "quit" | "exit" => Some(SlashCommand::Bye),
        "h" | "home" => Some(SlashCommand::Home),
        "m" | "switch" => Some(SlashCommand::Mode),
        "models" => Some(SlashCommand::Model),
        _ => None,
    }
}

/// Get help text for all available commands
pub fn get_help_text() -> String {
    let mut help = String::from("Available commands:\n\n");
    
    for (command_str, command) in built_in_slash_commands() {
        help.push_str(&format!("/{} - {}\n", command_str, command.description()));
    }
    
    help.push_str("\nYou can also use aliases like /q for /bye, /h for /home, /m for /mode, /models for /model");
    
    help
}

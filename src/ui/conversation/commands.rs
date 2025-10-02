use std::str::FromStr;

use crate::events::BindrMode;

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

pub fn command_entries() -> Vec<CommandEntry> {
    SlashCommand::iter()
        .map(|command| CommandEntry {
            command,
            keyword: command.command(),
            description: command.description(),
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCommand {
    pub command: SlashCommand,
    pub argument: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandEntry {
    pub command: SlashCommand,
    pub keyword: &'static str,
    pub description: &'static str,
}

impl ParsedCommand {
    pub fn argument(&self) -> Option<&str> {
        self.argument.as_deref()
    }

    pub fn mode_target(&self) -> Option<BindrMode> {
        if self.command != SlashCommand::Mode {
            return None;
        }

        let arg = self.argument()?.trim().to_lowercase();
        match arg.as_str() {
            "b" | "brainstorm" => Some(BindrMode::Brainstorm),
            "p" | "plan" => Some(BindrMode::Plan),
            "e" | "execute" | "build" => Some(BindrMode::Execute),
            "d" | "doc" | "document" => Some(BindrMode::Document),
            _ => None,
        }
    }
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
pub fn parse_slash_command(input: &str) -> Option<ParsedCommand> {
    if !input.starts_with('/') {
        return None;
    }

    let mut parts = input[1..].split_whitespace();
    let head = parts.next()?;
    let rest: Vec<String> = parts.map(|s| s.to_string()).collect();

    let command = SlashCommand::from_str(head).ok().or_else(|| match head.to_lowercase().as_str() {
        "q" | "quit" | "exit" => Some(SlashCommand::Bye),
        "h" | "home" => Some(SlashCommand::Home),
        "m" | "switch" => Some(SlashCommand::Mode),
        "models" => Some(SlashCommand::Model),
        _ => None,
    })?;

    let argument = if rest.is_empty() {
        None
    } else {
        Some(rest.join(" "))
    };

    Some(ParsedCommand { command, argument })
}

/// Get help text for all available commands
pub fn get_help_text() -> String {
    let mut help = String::from("Available commands:\n\n");
    for (command_str, command) in built_in_slash_commands() {
        help.push_str(&format!("/{} - {}\n", command_str, command.description()));
    }
    
    help.push_str("\nYou can also use aliases like /q for /bye, /h for /home, /m for /mode, /models for /model");
    help.push_str("\nUse /mode <b|p|e|d> to jump directly to Brainstorm, Plan, Execute, or Document mode.");

    help
}

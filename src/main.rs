// src/main.rs
use clap::{Parser, Subcommand};

mod agent;
mod agents;
mod config;
mod core;
mod events;
mod llm;
mod prompts;
mod session;
mod streaming;
pub mod tools;
mod ui;

use crate::core::runtime;

#[derive(Parser)]
#[command(name = "bindr")]
#[command(version = "0.1.0")]
#[command(about = "Multi-agent LLM workflow orchestration", long_about = None)]
struct Cli {
    /// Enable detailed structured logs
    #[arg(long, global = true)]
    verbose: bool,
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    runtime::init_logging(cli.verbose)?;

    match cli.command {
        None => {
            if let Err(e) = runtime::run_tui().await {
                eprintln!("Error running TUI: {e}");
                std::process::exit(1);
            }
        }
        Some(Commands::List) => {
            runtime::list_projects().await?;
        }
        Some(Commands::Open { name }) => {
            runtime::open_project(&name).await?;
        }
    }

    Ok(())
}

use clap::{Parser, Subcommand};
use anyhow::Result;

mod storage;
mod commands;

use commands::{list_projects, open_project, start_brainstorm};

#[derive(Parser)]
#[command(name = "bindr")]
#[command(about = "Multi-agent LLM workflow orchestration for builders")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// List all existing projects
    List,
    /// Open an existing project
    Open {
        /// Name of the project to open
        project_name: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::List) => {
            list_projects().await?;
        }
        Some(Commands::Open { project_name }) => {
            open_project(&project_name).await?;
        }
        None => {
            // Default behavior: start brainstorm mode
            start_brainstorm().await?;
        }
    }

    Ok(())
}

use clap::{Parser, Subcommand};

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

fn main() {
    let cli = Cli::parse();

    match cli.command {
        None => {
            // bindr with no args = smart detection
            println!("🚀 Launching Bindr...");
            println!("(Smart directory detection coming soon!)");
        }
        Some(Commands::List) => {
            println!("📋 Your Bindr projects:");
            println!("(No projects yet)");
        }
        Some(Commands::Open { name }) => {
            println!("📂 Opening project: {}", name);
            println!("(Not implemented yet)");
        }
    }
}

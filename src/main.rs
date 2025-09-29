use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::fs;

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

fn get_bindr_projects_path() -> PathBuf {
    let home = dirs::home_dir().expect("Could not find home directory");
    home.join(".bindr").join("projects")
}

fn list_projects() {
    let projects_path = get_bindr_projects_path();
    
    // Check if the directory exists
    if !projects_path.exists() {
        println!("📭 No projects yet. Run 'bindr' to start your first project!");
        return;
    }
    
    // Try to read the directory
    match fs::read_dir(&projects_path) {
        Ok(entries) => {
            let projects: Vec<_> = entries
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.path().is_dir())
                .filter_map(|entry| entry.file_name().into_string().ok())
                .collect();
            
            if projects.is_empty() {
                println!("📭 No projects yet. Run 'bindr' to start your first project!");
            } else {
                println!("📋 Your Bindr projects:\n");
                for project in projects {
                    println!("  • {}", project);
                }
            }
        }
        Err(_) => {
            println!("📭 No projects yet. Run 'bindr' to start your first project!");
        }
    }
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
            list_projects();
        }
        Some(Commands::Open { name }) => {
            println!("📂 Opening project: {}", name);
            println!("(Not implemented yet)");
        }
    }
}
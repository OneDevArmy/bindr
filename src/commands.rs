use anyhow::{Context, Result};
use crate::storage::StorageManager;
use std::io::{self, Write};

pub async fn list_projects() -> Result<()> {
    let storage = StorageManager::new()?;
    let projects = storage.get_projects()?;

    if projects.is_empty() {
        println!("No projects found. Run 'bindr' to start brainstorming a new project!");
        return Ok(());
    }

    println!("📁 Your Bindr Projects:");
    println!("{}", "=".repeat(50));
    
    for project in projects {
        println!("📋 {}", project.name);
        println!("   📍 Path: {}", project.path.display());
        println!("   🕒 Created: {}", project.created_at);
        println!("   🎯 Mode: {}", project.current_mode);
        println!();
    }

    Ok(())
}

pub async fn open_project(project_name: &str) -> Result<()> {
    let storage = StorageManager::new()?;
    
    if !storage.project_exists(project_name) {
        println!("❌ Project '{}' not found.", project_name);
        println!("Run 'bindr list' to see available projects.");
        return Ok(());
    }

    let metadata = storage.get_project_metadata(project_name)?;
    
    println!("🚀 Opening project: {}", project_name);
    println!("📍 Project path: {}", metadata.path.display());
    println!("🎯 Current mode: {}", metadata.current_mode);
    println!();
    
    // For Phase 0, we'll just show the project info
    // In later phases, this will launch the TUI for the specific mode
    println!("✨ Project opened successfully!");
    println!("💡 In future versions, this will launch the TUI for {} mode.", metadata.current_mode);
    
    Ok(())
}

pub async fn start_brainstorm() -> Result<()> {
    println!("🧠 Welcome to Bindr Brainstorm Mode!");
    println!("{}", "=".repeat(50));
    println!();
    println!("Let's explore your ideas and turn them into reality!");
    println!();
    
    // For Phase 0, we'll create a simple interactive prompt
    // In later phases, this will launch the TUI with LLM integration
    print!("💭 What would you like to build? ");
    io::stdout().flush()?;
    
    let mut input = String::new();
    io::stdin().read_line(&mut input)
        .context("Failed to read user input")?;
    
    let idea = input.trim();
    
    if idea.is_empty() {
        println!("👋 No worries! Come back when you have an idea to explore.");
        return Ok(());
    }
    
    println!();
    println!("🎯 Great idea: {}", idea);
    println!();
    
    // Ask for project name
    print!("📝 What should we call this project? ");
    io::stdout().flush()?;
    
    let mut project_name = String::new();
    io::stdin().read_line(&mut project_name)
        .context("Failed to read project name")?;
    
    let project_name = project_name.trim();
    
    if project_name.is_empty() {
        println!("❌ Project name cannot be empty.");
        return Ok(());
    }
    
    // Ask for project location
    print!("📁 Where should we create the project? (press Enter for current directory): ");
    io::stdout().flush()?;
    
    let mut project_path = String::new();
    io::stdin().read_line(&mut project_path)
        .context("Failed to read project path")?;
    
    let project_path = if project_path.trim().is_empty() {
        std::env::current_dir()?.join(project_name)
    } else {
        std::path::PathBuf::from(project_path.trim()).join(project_name)
    };
    
    // Create the project
    let storage = StorageManager::new()?;
    storage.create_project(project_name, project_path.clone())?;
    
    println!();
    println!("🎉 Project '{}' created successfully!", project_name);
    println!("📍 Location: {}", project_path.display());
    println!();
    println!("💡 In future versions, this will:");
    println!("   1. Launch the TUI for brainstorming with an LLM");
    println!("   2. Help you refine your idea");
    println!("   3. Move to Plan mode when ready");
    println!();
    println!("For now, you can run:");
    println!("  bindr list     - See all your projects");
    println!("  bindr open {}  - Open this project", project_name);
    
    Ok(())
}

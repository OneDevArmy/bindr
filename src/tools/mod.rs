pub mod capabilities;
pub mod dispatcher;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::events::BindrMode;
pub use capabilities::ToolKind;
pub use dispatcher::ToolDispatcher;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInvocation {
    pub tool: BindrTool,
    pub mode: BindrMode,
    pub description: String,
}

impl ToolInvocation {
    pub fn new(tool: BindrTool, mode: BindrMode, description: impl Into<String>) -> Self {
        Self {
            tool,
            mode,
            description: description.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRequestOutcome {
    pub invocation: ToolInvocation,
    pub requires_approval: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BindrTool {
    ReadFile(ReadFileOptions),
    WriteFile(WriteFileOptions),
    ListDirectory(ListDirectoryOptions),
    DiffFile(DiffFileOptions),
    ApplyPatch(ApplyPatchOptions),
    RunCommand(CommandOptions),
    ListModels,
    SelectModel(ModelSelection),
}

impl BindrTool {
    pub fn kind(&self) -> ToolKind {
        match self {
            BindrTool::ReadFile(_) => ToolKind::ReadFile,
            BindrTool::WriteFile(_) => ToolKind::WriteFile,
            BindrTool::ListDirectory(_) => ToolKind::ListDirectory,
            BindrTool::DiffFile(_) => ToolKind::DiffFile,
            BindrTool::ApplyPatch(_) => ToolKind::ApplyPatch,
            BindrTool::RunCommand(_) => ToolKind::RunCommand,
            BindrTool::ListModels => ToolKind::ListModels,
            BindrTool::SelectModel(_) => ToolKind::SelectModel,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadFileOptions {
    pub path: PathBuf,
    pub max_bytes: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteFileOptions {
    pub path: PathBuf,
    pub contents: String,
    pub create_if_missing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListDirectoryOptions {
    pub path: PathBuf,
    pub recursive: bool,
    pub include_hidden: bool,
    pub max_entries: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffFileOptions {
    pub path: PathBuf,
    pub context_lines: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyPatchOptions {
    pub path: PathBuf,
    pub patch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandOptions {
    pub command: String,
    pub args: Vec<String>,
    pub working_dir: PathBuf,
    pub allow_network: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSelection {
    pub provider_id: String,
    pub model_id: String,
}

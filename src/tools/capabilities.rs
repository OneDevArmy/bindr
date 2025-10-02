use std::collections::HashMap;

use once_cell::sync::Lazy;

use crate::events::BindrMode;
#[derive(Debug, Clone)]
pub struct ModeCapabilities {
    pub allowed_tools: Vec<ToolKind>,
    pub auto_approve: Vec<ToolKind>,
    pub default_provider: Option<String>,
    pub default_model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ToolKind {
    ReadFile,
    ListDirectory,
    DiffFile,
    WriteFile,
    ApplyPatch,
    RunCommand,
    ListModels,
    SelectModel,
}

pub static MODE_CAPABILITIES: Lazy<HashMap<BindrMode, ModeCapabilities>> = Lazy::new(|| {
    use BindrMode::*;

    let mut map = HashMap::new();

    map.insert(
        Brainstorm,
        ModeCapabilities {
            allowed_tools: vec![
                ToolKind::ReadFile,
                ToolKind::ListDirectory,
                ToolKind::ListModels,
                ToolKind::SelectModel,
            ],
            auto_approve: vec![ToolKind::ReadFile, ToolKind::ListDirectory, ToolKind::ListModels],
            default_provider: None,
            default_model: None,
        },
    );

    map.insert(
        Plan,
        ModeCapabilities {
            allowed_tools: vec![
                ToolKind::ReadFile,
                ToolKind::ListDirectory,
                ToolKind::ListModels,
                ToolKind::SelectModel,
            ],
            auto_approve: vec![ToolKind::ReadFile, ToolKind::ListDirectory, ToolKind::ListModels],
            default_provider: None,
            default_model: None,
        },
    );

    map.insert(
        Execute,
        ModeCapabilities {
            allowed_tools: vec![
                ToolKind::ReadFile,
                ToolKind::ListDirectory,
                ToolKind::DiffFile,
                ToolKind::ApplyPatch,
                ToolKind::RunCommand,
                ToolKind::ListModels,
                ToolKind::SelectModel,
            ],
            auto_approve: vec![ToolKind::ReadFile, ToolKind::ListDirectory, ToolKind::DiffFile, ToolKind::ListModels],
            default_provider: None,
            default_model: None,
        },
    );

    map.insert(
        Document,
        ModeCapabilities {
            allowed_tools: vec![
                ToolKind::ReadFile,
                ToolKind::ListDirectory,
                ToolKind::WriteFile,
                ToolKind::DiffFile,
                ToolKind::ListModels,
                ToolKind::SelectModel,
            ],
            auto_approve: vec![ToolKind::ReadFile, ToolKind::ListDirectory, ToolKind::ListModels],
            default_provider: None,
            default_model: None,
        },
    );

    map
});

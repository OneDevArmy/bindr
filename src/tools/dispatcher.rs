use anyhow::{anyhow, Result};

use crate::events::BindrMode;

use super::capabilities::{ModeCapabilities, MODE_CAPABILITIES};
use super::{ToolInvocation, ToolRequestOutcome};

/// Validates and routes tool invocations according to the active mode's capabilities.
pub struct ToolDispatcher;

impl ToolDispatcher {
    pub fn review(mode: BindrMode, invocation: ToolInvocation) -> Result<ToolRequestOutcome> {
        let capabilities = Self::capabilities_for(mode)?;
        let kind = invocation.tool.kind();

        if !capabilities.allowed_tools.contains(&kind) {
            return Err(anyhow!(
                "Tool {:?} is not permitted in {:?} mode",
                kind,
                mode
            ));
        }

        let requires_approval = !capabilities.auto_approve.contains(&kind);

        Ok(ToolRequestOutcome {
            invocation,
            requires_approval,
        })
    }

    pub fn capabilities_for(mode: BindrMode) -> Result<&'static ModeCapabilities> {
        MODE_CAPABILITIES
            .get(&mode)
            .ok_or_else(|| anyhow!("No capabilities registered for mode {:?}", mode))
    }
}

use serde::{Deserialize, Serialize};

use crate::TargetId;

/// Read-only recovery advice shared by CLI, MCP, and dashboard projections.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GateRecovery {
    pub scope: String,
    pub inspection: String,
    pub preview_available: bool,
    pub execute: Vec<TargetId>,
    pub reuse: Vec<TargetId>,
    pub targets: Vec<TargetRecovery>,
    pub next_step: Option<RecoveryCommand>,
    pub message: String,
    pub legacy_tool_note: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TargetRecovery {
    pub target: TargetId,
    pub disposition: String,
    pub reason: String,
    pub refresh: Option<RecoveryCommand>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecoveryCommand {
    pub argv: Vec<String>,
    pub read_only: bool,
}

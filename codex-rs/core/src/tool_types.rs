//! Shared tool type definitions used by both HTTP-backend and shared modules.
//!
//! These enums were originally defined in `tools/` submodules but are needed by
//! always-compiled modules (`exec_policy`, `sandboxing`, `model_family`). They
//! live here so they remain available even when `tools/` is gated behind
//! `legacy-http-backend`.

use serde::Deserialize;
use serde::Serialize;

/// Specifies what tool orchestrator should do with a given tool call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ApprovalRequirement {
    /// No approval required for this tool call.
    Skip {
        /// The first attempt should skip sandboxing (e.g., when explicitly
        /// greenlit by policy).
        bypass_sandbox: bool,
    },
    /// Approval required for this tool call
    NeedsApproval { reason: Option<String> },
    /// Execution forbidden for this tool call
    Forbidden { reason: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SandboxablePreference {
    Auto,
    #[allow(dead_code)] // Will be used by later tools.
    Require,
    #[allow(dead_code)] // Will be used by later tools.
    Forbid,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ConfigShellToolType {
    Default,
    Local,
    UnifiedExec,
    /// Do not include a shell tool by default. Useful when using Codex
    /// with tools provided exclusively provided by MCP servers. Often used
    /// with `--config base_instructions=CUSTOM_INSTRUCTIONS`
    /// to customize agent behavior.
    Disabled,
    /// Takes a command as a single string to be run in the user's default shell.
    ShellCommand,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ApplyPatchToolType {
    Freeform,
    Function,
}

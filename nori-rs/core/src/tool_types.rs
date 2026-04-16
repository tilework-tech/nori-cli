//! Shared tool type definitions used by multiple modules.

use serde::Deserialize;
use serde::Serialize;

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

/// The special argv[1] sentinel that tells the binary to run as apply-patch.
pub const CODEX_APPLY_PATCH_ARG1: &str = "--codex-run-as-apply-patch";

use codex_core::protocol::AskForApproval;
use codex_core::protocol::SandboxPolicy;

/// A simple preset pairing an approval policy with a sandbox policy.
#[derive(Debug, Clone)]
pub struct ApprovalPreset {
    /// Stable identifier for the preset.
    pub id: &'static str,
    /// Display label shown in UIs.
    pub label: &'static str,
    /// Short human description shown next to the label in UIs.
    pub description: &'static str,
    /// Approval policy to apply.
    pub approval: AskForApproval,
    /// Sandbox policy to apply.
    pub sandbox: SandboxPolicy,
}

/// Built-in list of approval presets that pair approval and sandbox policy.
///
/// Keep this UI-agnostic so it can be reused by both TUI and MCP server.
pub fn builtin_approval_presets() -> Vec<ApprovalPreset> {
    vec![
        ApprovalPreset {
            id: "read-only",
            label: "Read Only",
            description: "Requires approval to edit files and run commands.",
            approval: AskForApproval::OnRequest,
            sandbox: SandboxPolicy::ReadOnly,
        },
        ApprovalPreset {
            id: "auto",
            label: "Agent",
            description: "Read and edit files, and run commands.",
            approval: AskForApproval::OnRequest,
            sandbox: SandboxPolicy::new_workspace_write_policy(),
        },
        ApprovalPreset {
            id: "full-access",
            label: "Agent (full access)",
            description: "Can edit files outside this workspace and run commands with network access. Exercise caution when using.",
            approval: AskForApproval::Never,
            sandbox: SandboxPolicy::DangerFullAccess,
        },
    ]
}

/// Returns the display label for the current approval mode based on matching
/// the given approval policy and sandbox policy against the builtin presets.
///
/// Returns a simplified label for display in the status line:
/// - "Read Only" for read-only mode
/// - "Agent" for agent mode with workspace write access
/// - "Full Access" for full access mode (simplified from "Agent (full access)")
///
/// Returns `None` if no preset matches the current configuration.
pub fn approval_mode_label(approval: AskForApproval, sandbox: &SandboxPolicy) -> Option<String> {
    builtin_approval_presets()
        .into_iter()
        .find(|preset| preset.approval == approval && sandbox_matches(&preset.sandbox, sandbox))
        .map(|preset| {
            // Simplify "Agent (full access)" to "Full Access"
            if preset.id == "full-access" {
                "Full Access".to_string()
            } else {
                preset.label.to_string()
            }
        })
}

/// Check if sandbox policies match, ignoring differences in writable_roots
/// for WorkspaceWrite policies.
fn sandbox_matches(preset_sandbox: &SandboxPolicy, current_sandbox: &SandboxPolicy) -> bool {
    matches!(
        (preset_sandbox, current_sandbox),
        (SandboxPolicy::ReadOnly, SandboxPolicy::ReadOnly)
            | (
                SandboxPolicy::DangerFullAccess,
                SandboxPolicy::DangerFullAccess
            )
            | (
                SandboxPolicy::WorkspaceWrite { .. },
                SandboxPolicy::WorkspaceWrite { .. }
            )
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn approval_mode_label_returns_read_only_for_read_only_preset() {
        let label = approval_mode_label(AskForApproval::OnRequest, &SandboxPolicy::ReadOnly);
        assert_eq!(label, Some("Read Only".to_string()));
    }

    #[test]
    fn approval_mode_label_returns_agent_for_workspace_write_preset() {
        let sandbox = SandboxPolicy::new_workspace_write_policy();
        let label = approval_mode_label(AskForApproval::OnRequest, &sandbox);
        assert_eq!(label, Some("Agent".to_string()));
    }

    #[test]
    fn approval_mode_label_returns_full_access_for_danger_full_access_preset() {
        let label = approval_mode_label(AskForApproval::Never, &SandboxPolicy::DangerFullAccess);
        assert_eq!(label, Some("Full Access".to_string()));
    }

    #[test]
    fn approval_mode_label_matches_workspace_write_with_extra_roots() {
        // When user has extra writable roots, it should still match "Agent"
        let sandbox = SandboxPolicy::WorkspaceWrite {
            writable_roots: vec![PathBuf::from("/tmp/extra")],
            network_access: false,
            exclude_tmpdir_env_var: false,
            exclude_slash_tmp: false,
        };
        let label = approval_mode_label(AskForApproval::OnRequest, &sandbox);
        assert_eq!(label, Some("Agent".to_string()));
    }

    #[test]
    fn approval_mode_label_returns_none_for_unmatched_config() {
        // A config that doesn't match any preset (e.g., Never approval with ReadOnly sandbox)
        let label = approval_mode_label(AskForApproval::Never, &SandboxPolicy::ReadOnly);
        assert_eq!(label, None);
    }

    #[test]
    fn preset_descriptions_do_not_contain_codex_branding() {
        for preset in builtin_approval_presets() {
            assert!(
                !preset.description.contains("Codex"),
                "Preset '{}' description should not contain Codex branding: {}",
                preset.id,
                preset.description
            );
        }
    }
}

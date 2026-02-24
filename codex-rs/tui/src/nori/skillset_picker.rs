//! Skillset picker component for switching between available skillsets.
//!
//! This module provides functionality for:
//! - Checking if the nori-skillsets CLI is available
//! - Listing available skillsets
//! - Building a picker UI for skillset selection
//! - Installing selected skillsets

use std::path::PathBuf;

use crate::app_event::AppEvent;
use crate::bottom_pane::SelectionAction;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::popup_consts::standard_popup_hint_line;

/// The command name for the nori-skillsets CLI.
const NORI_SKILLSETS_CMD: &str = "nori-skillsets";

/// Check if nori-skillsets command is available in PATH.
pub fn is_nori_skillsets_available() -> bool {
    which::which(NORI_SKILLSETS_CMD).is_ok()
}

/// List available skillsets by running `nori-skillsets list`.
///
/// Returns:
/// - `Ok(names)` with skillset names on success (exit code 0)
/// - `Err(message)` with stdout/stderr on failure (non-zero exit)
pub async fn list_skillsets() -> Result<Vec<String>, String> {
    let output = tokio::process::Command::new(NORI_SKILLSETS_CMD)
        .arg("list")
        .output()
        .await
        .map_err(|e| format!("Failed to run {NORI_SKILLSETS_CMD}: {e}"))?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let names: Vec<String> = stdout
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();
        Ok(names)
    } else {
        // Combine stdout and stderr for error message
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = if !stderr.is_empty() {
            stderr.to_string()
        } else if !stdout.is_empty() {
            stdout.to_string()
        } else {
            format!(
                "{NORI_SKILLSETS_CMD} list failed with exit code {}",
                output.status.code().unwrap_or(-1)
            )
        };
        Err(message)
    }
}

/// Install a skillset by running `nori-skillsets install <name>`.
///
/// Returns:
/// - `Ok(message)` with filtered stdout on success (last section for long output)
/// - `Err(message)` with error output on failure
pub async fn install_skillset(name: &str) -> Result<String, String> {
    let args = build_install_args(name);
    let output = tokio::process::Command::new(NORI_SKILLSETS_CMD)
        .args(&args)
        .output()
        .await
        .map_err(|e| format!("Failed to run {NORI_SKILLSETS_CMD} install: {e}"))?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(filter_install_output(&stdout, name))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let message = if !stderr.is_empty() {
            stderr.trim().to_string()
        } else if !stdout.is_empty() {
            stdout.trim().to_string()
        } else {
            format!(
                "Failed to install skillset '{name}' (exit code {})",
                output.status.code().unwrap_or(-1)
            )
        };
        Err(message)
    }
}

/// Switch to a skillset by running `nori-skillsets switch <name> --install-dir <dir>`.
///
/// Returns:
/// - `Ok(message)` with filtered stdout on success
/// - `Err(message)` with error output on failure
pub async fn switch_skillset(name: &str, install_dir: &std::path::Path) -> Result<String, String> {
    let args = build_switch_args(name, install_dir);
    let output = tokio::process::Command::new(NORI_SKILLSETS_CMD)
        .args(&args)
        .output()
        .await
        .map_err(|e| format!("Failed to run {NORI_SKILLSETS_CMD} switch: {e}"))?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(filter_install_output(&stdout, name))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let message = if !stderr.is_empty() {
            stderr.trim().to_string()
        } else if !stdout.is_empty() {
            stdout.trim().to_string()
        } else {
            format!(
                "Failed to switch to skillset '{name}' (exit code {})",
                output.status.code().unwrap_or(-1)
            )
        };
        Err(message)
    }
}

/// Create selection view parameters for the skillset picker.
///
/// # Arguments
/// * `skillset_names` - List of available skillset names
/// * `install_dir` - When `Some`, sends `SwitchSkillset` with the given directory;
///   when `None`, sends `InstallSkillset` for backward compatibility.
pub fn skillset_picker_params(
    skillset_names: Vec<String>,
    install_dir: Option<PathBuf>,
) -> SelectionViewParams {
    let items: Vec<SelectionItem> = skillset_names
        .into_iter()
        .map(|name| {
            let name_for_action = name.clone();
            let install_dir = install_dir.clone();

            // Create action that sends the appropriate skillset event
            let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
                if let Some(dir) = install_dir.clone() {
                    tx.send(AppEvent::SwitchSkillset {
                        name: name_for_action.clone(),
                        install_dir: dir,
                    });
                } else {
                    tx.send(AppEvent::InstallSkillset {
                        name: name_for_action.clone(),
                    });
                }
            })];

            SelectionItem {
                search_value: Some(name.clone()),
                name,
                description: None,
                is_current: false,
                actions,
                dismiss_on_select: true,
                ..Default::default()
            }
        })
        .collect();

    SelectionViewParams {
        title: Some("Select Skillset".to_string()),
        subtitle: Some("Install a skillset to customize Nori's capabilities".to_string()),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        // Search is disabled because skillset items don't populate `search_value`.
        is_searchable: false,
        ..Default::default()
    }
}

/// Filter install output to extract the meaningful message.
///
/// If stdout has more than 3 lines, splits on double newlines and takes the
/// first non-empty trimmed line from the last section. Otherwise, takes the
/// first non-empty trimmed line. Falls back to a default message.
fn filter_install_output(stdout: &str, name: &str) -> String {
    let line_count = stdout.lines().count();

    let section = if line_count > 3 {
        // For long output, take the section after the last "\n\n"
        if let Some(pos) = stdout.rfind("\n\n") {
            &stdout[pos..]
        } else {
            stdout
        }
    } else {
        stdout
    };

    section
        .lines()
        .map(str::trim)
        .find(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| format!("Skillset '{name}' installed successfully"))
}

/// Build the argument list for the `nori-skillsets switch` command.
///
/// Extracted for testability.
fn build_switch_args(name: &str, install_dir: &std::path::Path) -> Vec<String> {
    vec![
        "--non-interactive".to_string(),
        "switch".to_string(),
        name.to_string(),
        "--install-dir".to_string(),
        install_dir.to_string_lossy().to_string(),
    ]
}

/// Build the argument list for the `nori-skillsets install` command.
///
/// Extracted for testability.
fn build_install_args(name: &str) -> Vec<String> {
    vec![
        "--non-interactive".to_string(),
        "install".to_string(),
        name.to_string(),
    ]
}

/// Message shown when nori-skillsets is not installed.
pub fn not_installed_message() -> String {
    "nori-skillsets is not installed. Install it with: `npm i -g nori-skillsets`".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_event::AppEvent;
    use crate::app_event_sender::AppEventSender;
    use pretty_assertions::assert_eq;
    use tokio::sync::mpsc::unbounded_channel;

    #[test]
    fn test_skillset_picker_params_creates_items() {
        let names = vec![
            "rust-dev".to_string(),
            "python-ml".to_string(),
            "web-frontend".to_string(),
        ];

        let params = skillset_picker_params(names, None);

        assert!(params.title.is_some());
        assert!(params.title.unwrap().contains("Skillset"));
        assert_eq!(params.items.len(), 3);
        assert_eq!(params.items[0].name, "rust-dev");
        assert_eq!(params.items[1].name, "python-ml");
        assert_eq!(params.items[2].name, "web-frontend");
    }

    #[test]
    fn test_skillset_picker_params_empty_list() {
        let names: Vec<String> = vec![];
        let params = skillset_picker_params(names, None);

        assert!(params.items.is_empty());
    }

    #[test]
    fn test_skillset_picker_items_dismiss_on_select() {
        let names = vec!["test".to_string()];
        let params = skillset_picker_params(names, None);

        assert!(params.items[0].dismiss_on_select);
    }

    #[test]
    fn test_skillset_picker_action_sends_install_event() {
        let names = vec!["my-skillset".to_string()];
        let params = skillset_picker_params(names, None);

        let (tx_raw, mut rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);

        // Execute the action
        assert!(!params.items[0].actions.is_empty());
        (params.items[0].actions[0])(&tx);

        // Check that the correct event was sent
        let event = rx.try_recv().expect("Should have received an event");
        match event {
            AppEvent::InstallSkillset { name } => {
                assert_eq!(name, "my-skillset");
            }
            _ => panic!("Expected InstallSkillset event"),
        }
    }

    #[test]
    fn test_skillset_picker_with_install_dir_sends_switch_event() {
        let names = vec!["my-skillset".to_string()];
        let dir = PathBuf::from("/tmp/worktree");
        let params = skillset_picker_params(names, Some(dir));

        let (tx_raw, mut rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);

        // Execute the action
        assert!(!params.items[0].actions.is_empty());
        (params.items[0].actions[0])(&tx);

        // Check that the correct event was sent
        let event = rx.try_recv().expect("Should have received an event");
        match event {
            AppEvent::SwitchSkillset { name, install_dir } => {
                assert_eq!(name, "my-skillset");
                assert_eq!(install_dir, PathBuf::from("/tmp/worktree"));
            }
            _ => panic!("Expected SwitchSkillset event, got {event:?}"),
        }
    }

    #[test]
    fn test_skillset_picker_items_have_search_value() {
        // Each item must have search_value set so the searchable picker can
        // filter items when the user types. Without search_value, typing
        // any character causes all items to be filtered out.
        let names = vec![
            "rust-dev".to_string(),
            "python-ml".to_string(),
            "web-frontend".to_string(),
        ];
        let params = skillset_picker_params(names, None);

        for item in &params.items {
            assert_eq!(
                item.search_value.as_deref(),
                Some(item.name.as_str()),
                "item '{}' must have search_value set to its name for filtering to work",
                item.name
            );
        }
    }

    #[test]
    fn test_switch_skillset_command_includes_non_interactive() {
        // The switch command must include --non-interactive since the TUI
        // captures stdout/stderr and provides no stdin. Without it, the
        // CLI prompts for confirmation and hangs forever.
        // --non-interactive must precede the subcommand for correct parsing.
        assert_eq!(
            build_switch_args("my-skillset", std::path::Path::new("/tmp/worktree")),
            vec![
                "--non-interactive",
                "switch",
                "my-skillset",
                "--install-dir",
                "/tmp/worktree",
            ]
        );
    }

    #[test]
    fn test_install_skillset_command_includes_non_interactive() {
        // Same as switch: install must not prompt interactively.
        // --non-interactive must precede the subcommand for correct parsing.
        assert_eq!(
            build_install_args("my-skillset"),
            vec!["--non-interactive", "install", "my-skillset"]
        );
    }

    #[test]
    fn test_not_installed_message() {
        let msg = not_installed_message();
        assert!(msg.contains("npm i -g nori-skillsets"));
    }

    #[test]
    fn test_filter_short_output_three_lines() {
        let stdout = "Line one\nLine two\nLine three\n";
        let result = filter_install_output(stdout, "test");
        assert_eq!(result, "Line one");
    }

    #[test]
    fn test_filter_long_output_takes_last_section() {
        let stdout = "Setting up Nori...\nDoing stuff\nMore stuff\nEven more\n\nSkillset \"test\" is now active.\nRestart Claude Code to apply.\n";
        let result = filter_install_output(stdout, "test");
        assert_eq!(result, "Skillset \"test\" is now active.");
    }

    #[test]
    fn test_filter_long_output_no_double_newline_falls_back() {
        let stdout = "Line one\nLine two\nLine three\nLine four\n";
        let result = filter_install_output(stdout, "test");
        assert_eq!(result, "Line one");
    }

    #[test]
    fn test_filter_empty_output_returns_default() {
        let stdout = "";
        let result = filter_install_output(stdout, "my-skillset");
        assert_eq!(result, "Skillset 'my-skillset' installed successfully");
    }

    #[test]
    fn test_filter_real_world_example() {
        let stdout = r#"Setting up Nori for first time use...

Warning: ⚠️  Nori managed installation detected in ancestor directory!

Claude Code loads CLAUDE.md files from all parent directories.
Having multiple Nori managed installations can cause duplicate or conflicting configurations.

Existing Nori managed installations found at:
  • /home/clifford/Documents/source/nori/registrar
  • /home/clifford/Documents/source/nori

Please remove the conflicting managed installation before continuing.

✓ Nori initialized successfully
Error: You do not have access to organization "dev".

Cannot download "dev/test-onboard" from https://dev.noriskillsets.dev.

Your available organizations: org-alpha, org-beta
Warning: Skillset "dev/test-onboard" not found in registry. Using locally installed version.
Switched to "dev/test-onboard" profile for Claude Code
Restart Claude Code to load the new profile configuration

Skillset "dev/test-onboard" is now active.
Restart Claude Code to apply the new skillset."#;
        let result = filter_install_output(stdout, "dev/test-onboard");
        assert_eq!(result, r#"Skillset "dev/test-onboard" is now active."#);
    }

    #[test]
    fn test_filter_long_output_last_section_has_empty_first_line() {
        let stdout = "Line 1\nLine 2\nLine 3\nLine 4\n\n\nActual message here.\n";
        let result = filter_install_output(stdout, "test");
        assert_eq!(result, "Actual message here.");
    }

    #[test]
    fn test_parse_skillset_names() {
        // Test the parsing logic used in list_skillsets
        let stdout = "rust-dev\npython-ml\n  web-frontend  \n\njava-backend\n";
        let names: Vec<String> = stdout
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();

        assert_eq!(names.len(), 4);
        assert_eq!(names[0], "rust-dev");
        assert_eq!(names[1], "python-ml");
        assert_eq!(names[2], "web-frontend");
        assert_eq!(names[3], "java-backend");
    }
}

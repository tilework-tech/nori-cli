//! Skillset picker component for switching between available skillsets.
//!
//! This module provides functionality for:
//! - Checking if the nori-skillsets CLI is available
//! - Listing available skillsets
//! - Building a picker UI for skillset selection
//! - Installing selected skillsets

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

/// List available skillsets by running `nori-skillsets list-skillsets`.
///
/// Returns:
/// - `Ok(names)` with skillset names on success (exit code 0)
/// - `Err(message)` with stdout/stderr on failure (non-zero exit)
pub async fn list_skillsets() -> Result<Vec<String>, String> {
    let output = tokio::process::Command::new(NORI_SKILLSETS_CMD)
        .arg("list-skillsets")
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
                "{NORI_SKILLSETS_CMD} list-skillsets failed with exit code {}",
                output.status.code().unwrap_or(-1)
            )
        };
        Err(message)
    }
}

/// Install a skillset by running `nori-skillsets install <name>`.
///
/// Returns:
/// - `Ok(first_line)` with first line of stdout on success
/// - `Err(message)` with error output on failure
pub async fn install_skillset(name: &str) -> Result<String, String> {
    let output = tokio::process::Command::new(NORI_SKILLSETS_CMD)
        .arg("install")
        .arg(name)
        .output()
        .await
        .map_err(|e| format!("Failed to run {NORI_SKILLSETS_CMD} install: {e}"))?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let first_line = stdout
            .lines()
            .next()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .unwrap_or_else(|| format!("Skillset '{name}' installed successfully"));
        Ok(first_line)
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

/// Create selection view parameters for the skillset picker.
///
/// # Arguments
/// * `skillset_names` - List of available skillset names
/// * `_app_event_tx` - The app event sender (used in actions)
pub fn skillset_picker_params(skillset_names: Vec<String>) -> SelectionViewParams {
    let items: Vec<SelectionItem> = skillset_names
        .into_iter()
        .map(|name| {
            let name_for_action = name.clone();

            // Create action that sends the install skillset event
            let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
                tx.send(AppEvent::InstallSkillset {
                    name: name_for_action.clone(),
                });
            })];

            SelectionItem {
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
        is_searchable: true,
        search_placeholder: Some("Search skillsets...".to_string()),
        ..Default::default()
    }
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
    use tokio::sync::mpsc::unbounded_channel;

    #[test]
    fn test_skillset_picker_params_creates_items() {
        let names = vec![
            "rust-dev".to_string(),
            "python-ml".to_string(),
            "web-frontend".to_string(),
        ];

        let params = skillset_picker_params(names);

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
        let params = skillset_picker_params(names);

        assert!(params.items.is_empty());
    }

    #[test]
    fn test_skillset_picker_is_searchable() {
        let names = vec!["test".to_string()];
        let params = skillset_picker_params(names);

        assert!(params.is_searchable);
        assert!(params.search_placeholder.is_some());
    }

    #[test]
    fn test_skillset_picker_items_dismiss_on_select() {
        let names = vec!["test".to_string()];
        let params = skillset_picker_params(names);

        assert!(params.items[0].dismiss_on_select);
    }

    #[test]
    fn test_skillset_picker_action_sends_install_event() {
        let names = vec!["my-skillset".to_string()];
        let params = skillset_picker_params(names);

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
    fn test_not_installed_message() {
        let msg = not_installed_message();
        assert!(msg.contains("npm i -g nori-skillsets"));
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

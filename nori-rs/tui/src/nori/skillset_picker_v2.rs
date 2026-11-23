//! Skillset picker for skillsets v2.
//!
//! Lists the skillsets layered into, or published for, the session's skillset
//! project and toggles one per selection: a layered skillset is unlayered, any
//! other is layered on top of the stack. The skillsets v2 calls themselves
//! live in the `nori-skillsets` crate.

use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use nori_skillsets::LayerChange;
use nori_skillsets::SkillsetList;
use nori_skillsets::SkillsetsCli;
use nori_skillsets::SkillsetsError;

use crate::app_event::AppEvent;
use crate::bottom_pane::SelectionAction;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;

/// Where the session's skillsets are layered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillsetProject {
    /// The session's own `.worktrees/<name>` worktree, so the compiled files
    /// stay isolated to this session.
    Worktree(PathBuf),
    /// The home directory: skillsets v2's machine-wide scope, used when the
    /// session has no worktree so the repo root is never compiled into.
    Home(PathBuf),
}

impl SkillsetProject {
    pub fn path(&self) -> &Path {
        match self {
            SkillsetProject::Worktree(path) | SkillsetProject::Home(path) => path,
        }
    }
}

/// Resolve the skillset project for a session running in `cwd`.
///
/// Worktree detection is path-based (`extract_worktree_name`): it recognizes
/// the `.worktrees/<name>` layout that auto-worktree creates. Anything else,
/// including the repo root, falls back to the home directory.
pub fn session_skillset_project(cwd: &Path, home: Option<PathBuf>) -> Option<SkillsetProject> {
    if crate::system_info::extract_worktree_name(cwd).is_some() {
        return Some(SkillsetProject::Worktree(cwd.to_path_buf()));
    }
    home.map(SkillsetProject::Home)
}

/// Skillset project for `cwd` using the current user's home directory.
pub fn current_skillset_project(cwd: &Path) -> Option<SkillsetProject> {
    session_skillset_project(cwd, dirs::home_dir())
}

/// Longest wait for a listing, which may query a remote repository.
const LIST_TIMEOUT: Duration = Duration::from_secs(60);
/// Longest wait for a layer or unlayer, which may fetch a release.
const CHANGE_TIMEOUT: Duration = Duration::from_secs(180);

/// List skillsets for `project` on a blocking thread.
pub async fn list_skillsets(project: PathBuf) -> Result<SkillsetList, String> {
    run_blocking(LIST_TIMEOUT, move |cli| cli.list(&project)).await
}

/// Layer or unlayer `name` in `project` on a blocking thread.
pub async fn change_skillset(
    project: PathBuf,
    name: String,
    layer: bool,
) -> Result<LayerChange, String> {
    run_blocking(CHANGE_TIMEOUT, move |cli| {
        if layer {
            cli.layer(&project, &name)
        } else {
            cli.unlayer(&project, &name)
        }
    })
    .await
}

/// Run a skillsets call on a blocking thread, giving up after `timeout` so a
/// stuck child process can never stall the session (the per-session startup
/// waits on these results before starting the agent).
async fn run_blocking<T: Send + 'static>(
    timeout: Duration,
    call: impl FnOnce(&SkillsetsCli) -> Result<T, SkillsetsError> + Send + 'static,
) -> Result<T, String> {
    let task = tokio::task::spawn_blocking(move || call(&SkillsetsCli::resolve()?));
    tokio::time::timeout(timeout, task)
        .await
        .map_err(|_| format!("skillsets CLI did not finish within {}s", timeout.as_secs()))?
        .map_err(|error| format!("skillsets task failed: {error}"))?
        .map_err(|error| error.to_string())
}

/// Message for a listing that found nothing to show.
pub fn empty_list_message(list: &SkillsetList) -> String {
    match &list.repository {
        Some(repository) => format!("No skillsets are published in {repository}."),
        None => "No skillsets repository is configured. Set one with \
                 `nori-skillsets config set repository --value <git-url-or-checkout>`."
            .to_string(),
    }
}

/// Create selection view parameters for the skillset picker.
///
/// * `show_no_skillset` - prepend a "No Skillset" row (per-session startup)
///   that dismisses the picker without changing anything.
/// * `auto_worktree_off` - per-session is on but auto-worktree is disabled, so
///   the subtitle warns where the files will land.
pub fn skillset_picker_params(
    list: SkillsetList,
    project: SkillsetProject,
    on_dismiss: Option<SelectionAction>,
    show_no_skillset: bool,
    auto_worktree_off: bool,
) -> SelectionViewParams {
    let mut items: Vec<SelectionItem> = Vec::new();

    if show_no_skillset {
        let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
            tx.send(AppEvent::SkillsetPickerDismissed);
        })];
        items.push(SelectionItem {
            name: "No Skillset".to_string(),
            description: Some("Start without a skillset".to_string()),
            actions,
            dismiss_on_select: true,
            ..Default::default()
        });
    }

    items.extend(list.skillsets.into_iter().map(|entry| {
        let name = entry.name.clone();
        let project_path = project.path().to_path_buf();
        let layer = !entry.layered;
        let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
            tx.send(AppEvent::ChangeSkillset {
                name: name.clone(),
                project: project_path.clone(),
                layer,
            });
        })];
        let version = entry.version.map(|version| format!("v{version}"));
        let (display_name, description) = if entry.layered {
            let description = match version {
                Some(version) => format!("{version} · select to unlayer"),
                None => "select to unlayer".to_string(),
            };
            (format!("{} (layered)", entry.name), Some(description))
        } else {
            (entry.name.clone(), version)
        };
        SelectionItem {
            search_value: Some(entry.name),
            name: display_name,
            description,
            actions,
            dismiss_on_select: true,
            ..Default::default()
        }
    }));

    let subtitle = match (&project, auto_worktree_off) {
        (SkillsetProject::Worktree(dir), true) => {
            format!("Warning: skillset files will be added to {}", dir.display())
        }
        (SkillsetProject::Worktree(dir), false) => {
            format!("Layering skillsets in {}", dir.display())
        }
        (SkillsetProject::Home(dir), true) => format!(
            "No session worktree - layering machine-wide into {}",
            dir.display()
        ),
        (SkillsetProject::Home(dir), false) => {
            format!("Layering skillsets machine-wide into {}", dir.display())
        }
    };

    SelectionViewParams {
        title: Some("Select skillset".to_string()),
        subtitle: Some(subtitle),
        footer_hint: None,
        items,
        is_searchable: true,
        search_placeholder: Some("Type to filter".to_string()),
        on_dismiss,
        ..Default::default()
    }
    .picker()
}

#[cfg(test)]
mod tests;

use super::*;
use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::ListSelectionView;
use crate::render::renderable::Renderable;
use insta::assert_snapshot;
use nori_skillsets::SkillsetEntry;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use tokio::sync::mpsc::unbounded_channel;

fn entry(name: &str, layered: bool, version: &str) -> SkillsetEntry {
    SkillsetEntry {
        name: name.to_string(),
        layered,
        version: Some(version.to_string()),
    }
}

fn list(skillsets: Vec<SkillsetEntry>) -> SkillsetList {
    SkillsetList {
        project: PathBuf::from("/tmp/worktree"),
        repository: Some("git@example.com:team/skillsets.git".to_string()),
        skillsets,
        warning: None,
    }
}

fn worktree() -> SkillsetProject {
    SkillsetProject::Worktree(PathBuf::from("/tmp/worktree"))
}

/// Run a picker row's action and return the event it sent.
fn select(params: &SelectionViewParams, index: usize) -> AppEvent {
    let (tx_raw, mut rx) = unbounded_channel::<AppEvent>();
    let tx = AppEventSender::new(tx_raw);
    (params.items[index].actions[0])(&tx);
    rx.try_recv().expect("row should send an event")
}

fn render(params: SelectionViewParams, width: u16) -> String {
    let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
    let view = ListSelectionView::new(params, AppEventSender::new(tx_raw));
    let height = view.desired_height(width);
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    view.render(area, &mut buf);
    (0..area.height)
        .map(|row| {
            (0..area.width)
                .map(|col| buf[(col, row)].symbol().to_string())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn project_is_the_session_worktree() {
    let cwd = PathBuf::from("/home/user/project/.worktrees/swift-oak");
    assert_eq!(
        session_skillset_project(&cwd, Some(PathBuf::from("/home/user"))),
        Some(SkillsetProject::Worktree(cwd.clone()))
    );
}

#[test]
fn project_is_home_outside_a_worktree() {
    // Regression: a non-worktree cwd (e.g. the repo root) must never be
    // compiled into, which would scatter skillset files into the project tree.
    let cwd = PathBuf::from("/home/user/project");
    assert_eq!(
        session_skillset_project(&cwd, Some(PathBuf::from("/home/user"))),
        Some(SkillsetProject::Home(PathBuf::from("/home/user")))
    );
}

#[test]
fn selecting_an_unlayered_skillset_layers_it() {
    let params = skillset_picker_params(
        list(vec![entry("rust-dev", false, "1.0.0")]),
        worktree(),
        None,
        false,
        false,
    );

    let event = select(&params, 0);

    let AppEvent::ChangeSkillset {
        name,
        project,
        layer,
    } = event
    else {
        panic!("expected ChangeSkillset, got {event:?}");
    };
    assert_eq!(
        (name.as_str(), project, layer),
        ("rust-dev", PathBuf::from("/tmp/worktree"), true)
    );
}

#[test]
fn selecting_a_layered_skillset_unlayers_it() {
    let params = skillset_picker_params(
        list(vec![entry("team", true, "1.0.0")]),
        worktree(),
        None,
        false,
        false,
    );

    let event = select(&params, 0);

    let AppEvent::ChangeSkillset {
        name,
        project,
        layer,
    } = event
    else {
        panic!("expected ChangeSkillset, got {event:?}");
    };
    assert_eq!(
        (name.as_str(), project, layer),
        ("team", PathBuf::from("/tmp/worktree"), false)
    );
}

#[test]
fn no_skillset_row_dismisses_the_picker() {
    let params = skillset_picker_params(
        list(vec![entry("rust-dev", false, "1.0.0")]),
        worktree(),
        None,
        true,
        false,
    );

    assert_eq!(params.items[0].name, "No Skillset");
    assert!(params.items[0].dismiss_on_select);
    let event = select(&params, 0);
    assert!(
        matches!(event, AppEvent::SkillsetPickerDismissed),
        "expected SkillsetPickerDismissed, got {event:?}"
    );
}

#[test]
fn rows_filter_by_skillset_name() {
    let params = skillset_picker_params(
        list(vec![
            entry("team", true, "1.0.0"),
            entry("rust-dev", false, "1.0.0"),
        ]),
        worktree(),
        None,
        false,
        false,
    );

    let search_values: Vec<Option<String>> = params
        .items
        .iter()
        .map(|item| item.search_value.clone())
        .collect();
    assert_eq!(
        search_values,
        vec![Some("team".to_string()), Some("rust-dev".to_string())]
    );
}

#[test]
fn empty_list_message_explains_how_to_configure_a_repository() {
    let mut unconfigured = list(vec![]);
    unconfigured.repository = None;

    assert!(
        empty_list_message(&unconfigured).contains("config set repository"),
        "{}",
        empty_list_message(&unconfigured)
    );
    assert_eq!(
        empty_list_message(&list(vec![])),
        "No skillsets are published in git@example.com:team/skillsets.git."
    );
}

#[test]
fn snapshot_skillset_picker_in_worktree() {
    let params = skillset_picker_params(
        list(vec![
            entry("team", true, "1.2.0"),
            entry("rust-dev", false, "0.3.0"),
            entry("web-frontend", false, "2.0.0-rc.1"),
        ]),
        worktree(),
        None,
        true,
        false,
    );
    assert_snapshot!("skillset_picker_in_worktree", render(params, 80));
}

#[test]
fn snapshot_skillset_picker_machine_wide_without_worktree() {
    let params = skillset_picker_params(
        list(vec![entry("rust-dev", false, "0.3.0")]),
        SkillsetProject::Home(PathBuf::from("/home/user")),
        None,
        true,
        true,
    );
    assert_snapshot!("skillset_picker_machine_wide", render(params, 80));
}

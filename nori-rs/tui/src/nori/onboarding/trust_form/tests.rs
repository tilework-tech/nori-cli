use std::path::PathBuf;

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use insta::assert_snapshot;
use nori_config::ProjectConfig;
use nori_config::ToolOutput;
use nori_config::TrustLevel;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use tempfile::TempDir;

use super::TrustForm;
use crate::nori::onboarding::KeyboardHandler;
use crate::nori::onboarding::StepState;
use crate::nori::onboarding::StepStateProvider;
use crate::nori::onboarding::TrustDirectorySelection;
use crate::nori::onboarding::welcome_step::Density;
use crate::nori::onboarding::welcome_step::WelcomeForm;

fn form(is_git_repo: bool, ask_tool_output: bool) -> (TrustForm, TempDir) {
    let home = TempDir::new().expect("temp NORI_HOME");
    let form = TrustForm::new(
        home.path().to_path_buf(),
        PathBuf::from("/workspace/project"),
        is_git_repo,
        ask_tool_output,
    );
    (form, home)
}

fn press(form: &mut TrustForm, codes: &[KeyCode]) {
    for code in codes {
        form.handle_key_event(KeyEvent::new(*code, KeyModifiers::NONE));
    }
}

fn render(form: &TrustForm, density: Density) -> String {
    let area = Rect::new(0, 0, 80, form.height(density));
    let mut buf = Buffer::empty(area);
    form.render_with(density, area, &mut buf);
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn saved(home: &TempDir) -> (Option<ProjectConfig>, Option<ToolOutput>) {
    let text = std::fs::read_to_string(home.path().join("config.toml")).unwrap_or_default();
    let config: nori_config::NoriConfigToml = toml::from_str(&text).expect("valid config");
    (
        config.projects.get("/workspace/project").cloned(),
        config.tui.tool_output,
    )
}

fn project(trust: TrustLevel, tool_output: Option<ToolOutput>) -> Option<ProjectConfig> {
    Some(ProjectConfig {
        trust_level: Some(trust),
        tool_output,
        tool_output_max_lines: None,
    })
}

#[test]
fn one_enter_trusts_a_git_repo_and_leaves_tool_output_unset() {
    let (mut form, home) = form(true, true);
    press(&mut form, &[KeyCode::Enter]);
    assert_eq!(
        (form.get_step_state(), form.selection(), saved(&home)),
        (
            StepState::Complete,
            Some(TrustDirectorySelection::Trust),
            (project(TrustLevel::Trusted, None), None)
        )
    );
}

#[test]
fn picking_full_saves_it_for_this_project() {
    let (mut form, home) = form(true, true);
    press(&mut form, &[KeyCode::Char('f'), KeyCode::Enter]);
    assert_eq!(
        saved(&home),
        (project(TrustLevel::Trusted, Some(ToolOutput::Full)), None)
    );
}

#[test]
fn a_folder_outside_version_control_asks_first_by_default() {
    let (mut form, home) = form(false, true);
    press(&mut form, &[KeyCode::Enter]);
    assert_eq!(
        (form.selection(), saved(&home)),
        (
            Some(TrustDirectorySelection::DontTrust),
            (project(TrustLevel::Untrusted, None), None)
        )
    );
}

#[test]
fn y_picks_trust_and_c_picks_compact_for_this_project_only() {
    let (mut form, home) = form(false, true);
    press(&mut form, &[KeyCode::Char('y'), KeyCode::Char('c')]);
    assert_eq!(form.get_step_state(), StepState::InProgress);
    press(&mut form, &[KeyCode::Enter]);
    assert_eq!(
        saved(&home),
        (
            project(TrustLevel::Trusted, Some(ToolOutput::Compact)),
            None
        )
    );
}

#[test]
fn arrows_move_between_answers_and_change_the_tool_output() {
    let (mut form, home) = form(true, true);
    press(
        &mut form,
        &[
            KeyCode::Down,
            KeyCode::Down,
            KeyCode::Right,
            KeyCode::Up,
            KeyCode::Enter,
        ],
    );
    assert_eq!(
        (form.selection(), saved(&home)),
        (
            Some(TrustDirectorySelection::DontTrust),
            (
                project(TrustLevel::Untrusted, Some(ToolOutput::Compact)),
                None
            )
        )
    );
}

#[test]
fn without_the_tool_output_question_only_trust_is_saved() {
    let (mut form, home) = form(true, false);
    press(&mut form, &[KeyCode::Down, KeyCode::Down, KeyCode::Enter]);
    assert_eq!(
        (form.selection(), saved(&home)),
        (
            Some(TrustDirectorySelection::DontTrust),
            (project(TrustLevel::Untrusted, None), None)
        )
    );
}

#[test]
fn release_events_are_ignored() {
    let (mut form, _home) = form(true, true);
    form.handle_key_event(KeyEvent {
        kind: KeyEventKind::Release,
        ..KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
    });
    assert_eq!(form.get_step_state(), StepState::InProgress);
}

#[test]
fn every_density_ends_on_its_last_row() {
    let (form, _home) = form(true, true);
    let got: Vec<(Density, bool)> = [Density::Full, Density::Compact, Density::Tight]
        .into_iter()
        .map(|d| {
            (
                d,
                render(&form, d)
                    .lines()
                    .last()
                    .is_some_and(|l| !l.is_empty()),
            )
        })
        .collect();
    assert_eq!(
        got,
        vec![
            (Density::Full, true),
            (Density::Compact, true),
            (Density::Tight, true)
        ]
    );
}

#[test]
fn trust_form_layouts() {
    let (git, _a) = form(true, true);
    let (plain, _b) = form(false, true);
    let (no_tool, _c) = form(true, false);
    let (mut compact, _d) = form(true, true);
    press(&mut compact, &[KeyCode::Char('c')]);
    assert_snapshot!("trust_git_repo", render(&git, Density::Full));
    assert_snapshot!(
        "trust_not_version_controlled",
        render(&plain, Density::Full)
    );
    assert_snapshot!("trust_without_tool_output", render(&no_tool, Density::Full));
    assert_snapshot!(
        "trust_compact_chosen_tight",
        render(&compact, Density::Tight)
    );
}

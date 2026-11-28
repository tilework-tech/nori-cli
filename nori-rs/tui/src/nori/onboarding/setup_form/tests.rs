use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use insta::assert_snapshot;
use nori_auth_providers::AuthStatus;
use nori_harness::AgentKind;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::WidgetRef;

use super::AgentChoice;
use super::Density;
use super::Permission;
use super::SetupChoices;
use super::SetupForm;
use super::save;
use crate::nori::onboarding::KeyboardHandler;

fn agents(signed_in: &[AgentKind]) -> Vec<AgentChoice> {
    [
        AgentKind::ClaudeCode,
        AgentKind::Codex,
        AgentKind::Gemini,
        AgentKind::OpenCode,
    ]
    .into_iter()
    .map(|kind| AgentChoice {
        kind,
        status: if signed_in.contains(&kind) {
            AuthStatus::SignedIn
        } else {
            AuthStatus::SignedOut
        },
    })
    .collect()
}

fn press(form: &mut SetupForm, codes: &[KeyCode]) {
    for code in codes {
        form.handle_key_event(KeyEvent::new(*code, KeyModifiers::NONE));
    }
}

fn render(form: &SetupForm, w: u16, h: u16) -> String {
    let area = Rect::new(0, 0, w, h);
    let mut buf = Buffer::empty(area);
    form.render_ref(area, &mut buf);
    text(&buf)
}

fn render_at(form: &SetupForm, density: Density, w: u16) -> String {
    let area = Rect::new(0, 0, w, form.height(density));
    let mut buf = Buffer::empty(area);
    form.render_with(density, area, &mut buf);
    text(&buf)
}

fn text(buf: &Buffer) -> String {
    let (w, h) = (buf.area.width, buf.area.height);
    (0..h)
        .map(|y| {
            (0..w)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn one_enter_accepts_the_signed_in_agent_and_the_defaults() {
    let mut form = SetupForm::new(agents(&[AgentKind::Codex]), None);
    press(&mut form, &[KeyCode::Enter]);
    assert_eq!(
        form.choices(),
        Some(SetupChoices {
            agent: AgentKind::Codex,
            permission: Permission::Agent,
            share_usage: true,
        })
    );
}

#[test]
fn the_configured_agent_wins_over_detection() {
    let mut form = SetupForm::new(agents(&[AgentKind::Codex]), Some(AgentKind::OpenCode));
    press(&mut form, &[KeyCode::Enter]);
    assert_eq!(form.choices().map(|c| c.agent), Some(AgentKind::OpenCode));
}

#[test]
fn an_agent_must_be_picked_when_none_is_signed_in() {
    let mut form = SetupForm::new(agents(&[]), None);
    press(
        &mut form,
        &[
            KeyCode::Down,
            KeyCode::Down,
            KeyCode::Down,
            KeyCode::Down,
            KeyCode::Enter,
        ],
    );
    assert_eq!(form.choices(), None);
    assert!(render(&form, 80, 16).contains("Pick an agent"));
    press(&mut form, &[KeyCode::Down, KeyCode::Enter]);
    assert_eq!(form.choices().map(|c| c.agent), Some(AgentKind::Codex));
}

#[test]
fn arrows_change_the_prefilled_answers() {
    let mut form = SetupForm::new(agents(&[AgentKind::ClaudeCode]), None);
    // Down past the four agents to permissions, left twice, down, toggle.
    press(
        &mut form,
        &[
            KeyCode::Down,
            KeyCode::Down,
            KeyCode::Down,
            KeyCode::Down,
            KeyCode::Left,
            KeyCode::Down,
            KeyCode::Right,
            KeyCode::Enter,
        ],
    );
    assert_eq!(
        form.choices(),
        Some(SetupChoices {
            agent: AgentKind::ClaudeCode,
            permission: Permission::Ask,
            share_usage: false,
        })
    );
}

#[test]
fn form_layout() {
    let form = SetupForm::new(agents(&[AgentKind::ClaudeCode, AgentKind::Codex]), None);
    assert_snapshot!(render(&form, 90, 16));
}

#[test]
fn saving_writes_only_what_differs_from_the_defaults() {
    let read = |home: &std::path::Path| {
        std::fs::read_to_string(home.join("config.toml")).unwrap_or_default()
    };
    let opt_out =
        |home: &std::path::Path| nori_installed::read_install_state(home).map(|s| s.opt_out);

    let home = tempfile::tempdir().expect("tempdir");
    save(
        home.path(),
        &SetupChoices {
            agent: AgentKind::Codex,
            permission: Permission::Agent,
            share_usage: true,
        },
    )
    .expect("save");
    let config = read(home.path());
    assert!(config.contains("agent = \"codex\""), "{config}");
    assert!(!config.contains("approval_policy"), "{config}");
    assert_eq!(opt_out(home.path()), None);

    let home = tempfile::tempdir().expect("tempdir");
    save(
        home.path(),
        &SetupChoices {
            agent: AgentKind::ClaudeCode,
            permission: Permission::Ask,
            share_usage: false,
        },
    )
    .expect("save");
    assert!(read(home.path()).contains("approval_policy = \"untrusted\""));
    assert_eq!(opt_out(home.path()), Some(true));
}

fn all_agents(signed_in: &[AgentKind]) -> Vec<AgentChoice> {
    AgentKind::all()
        .iter()
        .map(|kind| AgentChoice {
            kind: *kind,
            status: if signed_in.contains(kind) {
                AuthStatus::SignedIn
            } else {
                AuthStatus::SignedOut
            },
        })
        .collect()
}

#[test]
fn every_density_ends_on_its_last_row() {
    let form = SetupForm::new(all_agents(&[AgentKind::Codex]), None);
    let got: Vec<(Density, bool)> = [Density::Full, Density::Compact, Density::Tight]
        .into_iter()
        .map(|d| {
            (
                d,
                render_at(&form, d, 90)
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
fn compact_form_shows_three_agents_and_how_many_more() {
    let form = SetupForm::new(all_agents(&[AgentKind::Codex]), None);
    assert_snapshot!("compact_form", render_at(&form, Density::Compact, 90));
    assert_snapshot!("tight_form", render_at(&form, Density::Tight, 90));
}

#[test]
fn compact_agent_list_scrolls_with_the_cursor() {
    let mut form = SetupForm::new(all_agents(&[]), None);
    press(
        &mut form,
        &[KeyCode::Down, KeyCode::Down, KeyCode::Down, KeyCode::Down],
    );
    let shown = render_at(&form, Density::Compact, 90);
    let n = AgentKind::all().len();
    assert_eq!(
        [
            shown.contains(&format!("{} more", 2)),
            shown.contains(AgentKind::all()[4].display_name()),
            !shown.contains(AgentKind::all()[0].display_name()),
            shown.contains(&format!("{} more", n - 5)),
        ],
        [true, true, true, true],
        "{shown}"
    );
}

#[test]
fn compact_list_keeps_the_selected_agent_in_view_when_the_cursor_leaves_it() {
    let mut form = SetupForm::new(all_agents(&[AgentKind::all()[6]]), None);
    press(&mut form, &[KeyCode::Down; 2]);
    let after = render_at(&form, Density::Compact, 90);
    assert!(
        after.contains(AgentKind::all()[6].display_name()),
        "{after}"
    );
}

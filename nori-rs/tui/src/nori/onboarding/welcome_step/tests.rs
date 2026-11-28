use std::time::Duration;

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use nori_auth_providers::AuthStatus;
use nori_harness::AgentKind;
use nori_tui_components::onigiri::Caption;
use nori_tui_components::onigiri::Onigiri;
use nori_tui_components::onigiri::Tier;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;

use super::Density;
use super::Welcome;
use super::WelcomeForm;
use crate::nori::onboarding::setup_form::AgentChoice;
use crate::nori::onboarding::setup_step::NoriSetupStep;

const TALL: Rect = Rect::new(0, 0, 110, 44);
const SHORT: Rect = Rect::new(0, 0, 80, 24);

fn caption() -> Caption {
    Caption {
        version: "v0.42.0".to_string(),
        cwd: "~/tidepool".to_string(),
    }
}

fn welcome(tier: Tier, animate: bool) -> Welcome {
    let onigiri = Onigiri::new(tier, caption());
    Welcome::new(if animate { onigiri } else { onigiri.settled() })
}

fn step(animate: bool) -> NoriSetupStep {
    NoriSetupStep::setup(
        vec![AgentChoice {
            kind: AgentKind::Codex,
            status: AuthStatus::SignedIn,
        }],
        None,
        welcome(Tier::True, animate),
    )
}

fn landed() -> Duration {
    Onigiri::new(Tier::True, caption())
        .landed_at(step(true).split(TALL).0)
        .expect("the intro plays at 110x44")
}

/// Per form row: blank, drawn as the form draws itself (`x`), or the
/// same text all dimmed (`d`).
fn form_rows(step: &NoriSetupStep, area: Rect, at: Duration) -> Vec<char> {
    let mut buf = Buffer::empty(area);
    step.render_at(at, area, &mut buf);
    let (_, form) = step.split(area);
    let mut want = Buffer::empty(form);
    step.form.render_with(step.density(area), form, &mut want);
    (form.y..form.bottom())
        .map(|y| {
            let mut xs = form.x..form.right();
            if xs.clone().all(|x| buf[(x, y)].symbol() == " ") {
                ' '
            } else if xs.clone().all(|x| buf[(x, y)] == want[(x, y)]) {
                'x'
            } else if xs.all(|x| {
                buf[(x, y)].symbol() == want[(x, y)].symbol()
                    && (buf[(x, y)].symbol() == " " || buf[(x, y)].modifier.contains(Modifier::DIM))
            }) {
                'd'
            } else {
                '?'
            }
        })
        .collect()
}

fn is(rows: &[char], want: char) -> bool {
    rows.iter().all(|r| *r == want || *r == ' ') && rows.contains(&want)
}

#[test]
fn form_stays_hidden_until_the_ball_lands() {
    let s = step(true);
    let before = form_rows(&s, TALL, landed() - Duration::from_millis(1));
    assert!(before.iter().all(|r| *r == ' '), "{before:?}");
}

#[test]
fn form_unrolls_downwards_after_the_landing() {
    let s = step(true);
    let early = form_rows(&s, TALL, landed() + Duration::from_millis(40));
    let first = early.iter().position(|r| *r != ' ').expect("a row shows");
    let last = early.iter().rposition(|r| *r != ' ').expect("a row shows");
    assert_eq!(early[first], 'd', "{early:?}");
    assert!(
        form_rows(&s, TALL, landed() + Duration::from_secs(5))[last + 1..]
            .iter()
            .any(|r| *r != ' '),
        "lower rows are still hidden early on: {early:?}"
    );
    assert!(is(
        &form_rows(&s, TALL, landed() + Duration::from_secs(2)),
        'x'
    ));
}

#[test]
fn form_shows_at_once_without_the_intro() {
    assert!(is(&form_rows(&step(false), TALL, Duration::ZERO), 'x'));
    assert!(is(&form_rows(&step(true), SHORT, Duration::ZERO), 'x'));
}

#[test]
fn a_key_before_the_form_shows_reveals_it_and_is_not_applied() {
    let mut s = step(true);
    let early = Duration::from_millis(300);
    let _ = form_rows(&s, TALL, early);
    s.handle_key_at(early, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(s.choices(), None);
    assert!(is(&form_rows(&s, TALL, early), 'x'));
    s.handle_key_at(early, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(s.choices().is_some());
}

#[test]
fn frames_keep_coming_until_the_fade_ends_then_follow_the_welcome() {
    let s = step(true);
    let _ = form_rows(&s, TALL, Duration::ZERO);
    let during = landed() + Duration::from_millis(100);
    let after = landed() + Duration::from_millis(5010);
    let welcome = Onigiri::new(Tier::True, caption());
    assert_eq!(
        [
            s.next_frame_in_at(during, TALL)
                .is_some_and(|d| d <= Duration::from_millis(16)),
            s.next_frame_in_at(after, TALL) == welcome.next_frame_in(after, s.split(TALL).0),
        ],
        [true, true]
    );
}

fn all_agents_step() -> NoriSetupStep {
    NoriSetupStep::setup(
        AgentKind::all()
            .iter()
            .map(|kind| AgentChoice {
                kind: *kind,
                status: AuthStatus::SignedOut,
            })
            .collect(),
        None,
        welcome(Tier::Ansi16, false),
    )
}

fn screen(step: &NoriSetupStep, w: u16, h: u16) -> String {
    let area = Rect::new(0, 0, w, h);
    let mut buf = Buffer::empty(area);
    step.render_at(Duration::from_secs(10), area, &mut buf);
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
fn shorter_terminals_get_denser_forms() {
    use super::Density;
    use super::Fit;
    let s = all_agents_step();
    let got: Vec<((u16, u16), Fit)> = [
        (110, 44),
        (100, 30),
        (80, 24),
        (80, 16),
        (80, 12),
        (80, 9),
        (80, 8),
        (39, 30),
    ]
    .into_iter()
    .map(|(w, h)| ((w, h), s.fit(Rect::new(0, 0, w, h))))
    .collect();
    assert_eq!(
        got,
        vec![
            ((110, 44), Fit::Form(Density::Full)),
            ((100, 30), Fit::Form(Density::Compact)),
            ((80, 24), Fit::Form(Density::Compact)),
            ((80, 16), Fit::Form(Density::Compact)),
            ((80, 12), Fit::Form(Density::Tight)),
            ((80, 9), Fit::Form(Density::Tight)),
            ((80, 8), Fit::TooSmall),
            ((39, 30), Fit::TooSmall),
        ]
    );
}

#[test]
fn setup_screen_at_each_terminal_size() {
    let s = all_agents_step();
    for (w, h) in [(100, 30), (80, 24), (80, 16), (80, 12), (80, 9), (36, 8)] {
        insta::assert_snapshot!(format!("setup_screen_{w}x{h}"), screen(&s, w, h));
    }
}

#[test]
fn keys_do_nothing_while_the_terminal_is_too_small() {
    let mut s = step(false);
    let _ = screen(&s, 30, 6);
    s.handle_key_at(
        Duration::ZERO,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    );
    assert_eq!(s.choices(), None);
}

#[test]
fn a_form_that_takes_over_unrolls_when_first_drawn() {
    use crate::nori::onboarding::trust_form::NoriTrustStep;
    let shared = welcome(Tier::True, true);
    let trust = NoriTrustStep::trust(
        std::env::temp_dir(),
        std::path::PathBuf::from("/workspace/project"),
        true,
        true,
        shared,
        true,
    );
    let took_over = landed() + Duration::from_secs(3);
    let rows = |at: Duration| {
        let mut buf = Buffer::empty(TALL);
        trust.render_at(at, TALL, &mut buf);
        let (_, form) = trust.split(TALL);
        (form.y..form.bottom())
            .filter(|y| (form.x..form.right()).any(|x| buf[(x, *y)].symbol() != " "))
            .count()
    };
    let total = usize::from(trust.form.height(Density::Full)) - 2;
    assert_eq!(
        [rows(took_over), rows(took_over + Duration::from_secs(1))],
        [1, total]
    );
}

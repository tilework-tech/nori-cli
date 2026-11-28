//! The first-launch screen: the onigiri welcome above the setup form.
//!
//! When the welcome's intro plays, the form unrolls once the ball lands:
//! rows appear top-down, dim at first, then fully drawn. A key pressed
//! before the form is fully drawn reveals it at once and is not applied.

use std::cell::Cell;
use std::cell::RefCell;
use std::time::Duration;
use std::time::Instant;

use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use nori_harness::AgentKind;
use nori_tui_components::onigiri::Caption;
use nori_tui_components::onigiri::Onigiri;
use nori_tui_components::onigiri::Tier;
use nori_tui_components::onigiri::intro_fits;
use ratatui::buffer::Buffer;
use ratatui::layout::Alignment;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::WidgetRef;
use ratatui::widgets::Wrap;

use super::KeyboardHandler;
use super::StepState;
use super::StepStateProvider;
use super::setup_form::AgentChoice;
use super::setup_form::Density;
use super::setup_form::SetupChoices;
use super::setup_form::SetupForm;

/// Delay between one form row appearing and the next.
const FADE_ROW: Duration = Duration::from_millis(15);
/// How long each row stays dim before it is fully drawn.
const FADE_DIM: Duration = Duration::from_millis(120);
const FADE_FRAME: Duration = Duration::from_millis(16);

/// Narrowest terminal the setup screen draws in.
const MIN_WIDTH: u16 = 40;
/// Rows kept for the welcome above the form, at the least (its one-line
/// header).
const MIN_WELCOME: u16 = 1;
/// Rows the mini onigiri needs above the form.
const MINI_WELCOME: u16 = 3;

/// How the setup screen fits a terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fit {
    Form(Density),
    /// Too small to draw the form; a note asks for a bigger terminal.
    TooSmall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Hidden,
    Dim,
    Shown,
}

pub(crate) struct NoriSetupStep {
    form: SetupForm,
    welcome: RefCell<Onigiri>,
    started: Instant,
    /// When the form starts fading in, from the last drawn area; `None`
    /// shows it at once.
    fade_at: Cell<Option<Duration>>,
    /// Set by a key pressed before the form was fully drawn.
    skipped: bool,
    /// Whether the last drawn area was too small for the form.
    too_small: Cell<bool>,
}

/// Splits `area` into the welcome above and the form below.
fn split_for(area: Rect, form_h: u16) -> (Rect, Rect) {
    let form_h = form_h.min(area.height);
    let welcome = Rect {
        height: area.height - form_h,
        ..area
    };
    let form = Rect {
        y: area.y + welcome.height,
        height: form_h,
        ..area
    };
    (welcome, form)
}

impl NoriSetupStep {
    /// `animate` plays the welcome's intro; without it the settled welcome
    /// is drawn.
    pub(crate) fn new(
        agents: Vec<AgentChoice>,
        configured: Option<AgentKind>,
        caption: Caption,
        tier: Tier,
        animate: bool,
    ) -> Self {
        let welcome = Onigiri::new(tier, caption);
        Self {
            form: SetupForm::new(agents, configured),
            welcome: RefCell::new(if animate { welcome } else { welcome.settled() }),
            started: Instant::now(),
            fade_at: Cell::new(None),
            skipped: false,
            too_small: Cell::new(false),
        }
    }

    pub(crate) fn choices(&self) -> Option<SetupChoices> {
        self.form.choices()
    }

    /// The densest form needed: the full form when the welcome above it
    /// still plays its intro, else the compact one while the mini onigiri
    /// fits above it, else the tight one.
    fn fit(&self, area: Rect) -> Fit {
        let room = |d: Density| area.height.saturating_sub(self.form.height(d));
        if area.width < MIN_WIDTH || room(Density::Tight) < MIN_WELCOME {
            Fit::TooSmall
        } else if intro_fits(split_for(area, self.form.height(Density::Full)).0) {
            Fit::Form(Density::Full)
        } else if room(Density::Compact) >= MINI_WELCOME {
            Fit::Form(Density::Compact)
        } else {
            Fit::Form(Density::Tight)
        }
    }

    fn density(&self, area: Rect) -> Density {
        match self.fit(area) {
            Fit::Form(d) => d,
            Fit::TooSmall => Density::Tight,
        }
    }

    fn split(&self, area: Rect) -> (Rect, Rect) {
        split_for(area, self.form.height(self.density(area)))
    }

    fn row(&self, elapsed: Duration, index: u16) -> Row {
        let Some(at) = self.fade_at.get() else {
            return Row::Shown;
        };
        let start = at + FADE_ROW * u32::from(index);
        if elapsed < start {
            Row::Hidden
        } else if elapsed < start + FADE_DIM {
            Row::Dim
        } else {
            Row::Shown
        }
    }

    fn fully_shown(&self, elapsed: Duration) -> bool {
        let rows = self.form.height(Density::Full);
        self.fade_at.get().is_none_or(|at| {
            elapsed >= at + FADE_ROW * u32::from(rows.saturating_sub(1)) + FADE_DIM
        })
    }

    /// When the screen next needs a frame, if anything still moves.
    pub(crate) fn next_frame_in(&self, area: Rect) -> Option<Duration> {
        self.next_frame_in_at(self.started.elapsed(), area)
    }

    fn next_frame_in_at(&self, elapsed: Duration, area: Rect) -> Option<Duration> {
        let (welcome, _) = self.split(area);
        let next = self.welcome.borrow().next_frame_in(elapsed, welcome);
        if self.fully_shown(elapsed) {
            next
        } else {
            Some(next.map_or(FADE_FRAME, |n| n.min(FADE_FRAME)))
        }
    }

    fn handle_key_at(&mut self, elapsed: Duration, key_event: KeyEvent) {
        if self.too_small.get() {
            return;
        }
        if self.fully_shown(elapsed) {
            self.form.handle_key_event(key_event);
        } else if key_event.kind != KeyEventKind::Release {
            self.skipped = true;
            self.fade_at.set(None);
        }
    }

    fn render_at(&self, elapsed: Duration, area: Rect, buf: &mut Buffer) {
        let density = match self.fit(area) {
            Fit::Form(d) => d,
            Fit::TooSmall => {
                self.too_small.set(true);
                let note = format!(
                    "Make the terminal at least {MIN_WIDTH}×{} to set up Nori.",
                    self.form.height(Density::Tight) + MIN_WELCOME
                );
                let y = area.y + area.height / 2;
                let line = Rect {
                    y,
                    height: area.height - (y - area.y),
                    ..area
                };
                Paragraph::new(Line::from(note.dim()))
                    .alignment(Alignment::Center)
                    .wrap(Wrap { trim: true })
                    .render(line, buf);
                return;
            }
        };
        self.too_small.set(false);
        let (welcome_area, form_area) = self.split(area);
        let mut welcome = self.welcome.borrow_mut();
        if !self.skipped {
            self.fade_at.set(welcome.landed_at(welcome_area));
        }
        welcome.render(elapsed, welcome_area, buf);
        if self.fully_shown(elapsed) {
            self.form.render_with(density, form_area, buf);
            return;
        }
        let mut form = Buffer::empty(form_area);
        self.form.render_with(density, form_area, &mut form);
        for (i, y) in (form_area.y..form_area.bottom()).enumerate() {
            let row = self.row(elapsed, i as u16);
            if row == Row::Hidden {
                continue;
            }
            for x in form_area.x..form_area.right() {
                if !buf.area.contains((x, y).into()) {
                    continue;
                }
                let mut cell = form[(x, y)].clone();
                if row == Row::Dim {
                    cell.modifier.insert(Modifier::DIM);
                }
                buf[(x, y)] = cell;
            }
        }
    }
}

impl KeyboardHandler for NoriSetupStep {
    fn handle_key_event(&mut self, key_event: KeyEvent) {
        self.handle_key_at(self.started.elapsed(), key_event);
    }
}

impl StepStateProvider for NoriSetupStep {
    fn get_step_state(&self) -> StepState {
        self.form.get_step_state()
    }
}

impl WidgetRef for NoriSetupStep {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        self.render_at(self.started.elapsed(), area, buf);
    }
}

#[cfg(test)]
mod tests {
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

    use super::super::setup_form::AgentChoice;
    use super::NoriSetupStep;

    const TALL: Rect = Rect::new(0, 0, 110, 44);
    const SHORT: Rect = Rect::new(0, 0, 80, 24);

    fn caption() -> Caption {
        Caption {
            version: "v0.42.0".to_string(),
            cwd: "~/tidepool".to_string(),
        }
    }

    fn step(animate: bool) -> NoriSetupStep {
        NoriSetupStep::new(
            vec![AgentChoice {
                kind: AgentKind::Codex,
                status: AuthStatus::SignedIn,
            }],
            None,
            caption(),
            Tier::True,
            animate,
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
                        && (buf[(x, y)].symbol() == " "
                            || buf[(x, y)].modifier.contains(Modifier::DIM))
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
        NoriSetupStep::new(
            AgentKind::all()
                .iter()
                .map(|kind| AgentChoice {
                    kind: *kind,
                    status: AuthStatus::SignedOut,
                })
                .collect(),
            None,
            caption(),
            Tier::Ansi16,
            false,
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
        use super::super::setup_form::Density;
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
}

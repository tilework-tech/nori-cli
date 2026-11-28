//! The onigiri welcome above a form, shared by the first-launch setup and
//! the folder trust prompt.
//!
//! When the welcome's intro plays, the form unrolls once the ball lands:
//! rows appear top-down, dim at first, then fully drawn. A form that takes
//! over from an earlier one unrolls as soon as it does. A key pressed before
//! the form is fully drawn reveals it at once and is not applied.
//!
//! The form gets the densest layout the terminal needs ([`Density`]); below
//! [`MIN_WIDTH`] columns or the tight form's height a note asks for a bigger
//! terminal and keys do nothing.

#[cfg(test)]
mod tests;

use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;
use std::time::Instant;

use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use nori_tui_components::onigiri::Onigiri;
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

/// How much room a form takes. Shorter terminals get denser forms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Density {
    /// Everything listed, with blank rows between sections.
    Full,
    /// Long lists shortened, with blank rows between sections.
    Compact,
    /// Long lists shortened, no blank rows.
    Tight,
}

/// A form drawn under the welcome.
pub(crate) trait WelcomeForm: KeyboardHandler + StepStateProvider {
    /// Rows the form needs at `density`.
    fn height(&self, density: Density) -> u16;
    fn render_with(&self, density: Density, area: Rect, buf: &mut Buffer);
}

/// The onigiri and the clock it runs on, shared by consecutive steps so the
/// welcome carries on when one form replaces another.
#[derive(Clone)]
pub(crate) struct Welcome {
    onigiri: Rc<RefCell<Onigiri>>,
    started: Instant,
}

impl Welcome {
    pub(crate) fn new(onigiri: Onigiri) -> Self {
        Self {
            onigiri: Rc::new(RefCell::new(onigiri)),
            started: Instant::now(),
        }
    }
}

/// Delay between one form row appearing and the next.
const FADE_ROW: Duration = Duration::from_millis(15);
/// How long each row stays dim before it is fully drawn.
const FADE_DIM: Duration = Duration::from_millis(120);
const FADE_FRAME: Duration = Duration::from_millis(16);

/// Narrowest terminal the screen draws in.
const MIN_WIDTH: u16 = 40;
/// Rows kept for the welcome above the form, at the least (its one-line
/// header).
const MIN_WELCOME: u16 = 1;
/// Rows the mini onigiri needs above the form.
const MINI_WELCOME: u16 = 3;

/// How the screen fits a terminal.
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

pub(crate) struct WelcomeStep<F> {
    pub(crate) form: F,
    welcome: Welcome,
    /// Whether this step takes over from an earlier form; its form then
    /// unrolls when first drawn.
    follows: bool,
    /// When this step was first drawn.
    shown_at: Cell<Option<Duration>>,
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

impl<F: WelcomeForm> WelcomeStep<F> {
    pub(crate) fn new(form: F, welcome: Welcome, follows: bool) -> Self {
        Self {
            form,
            welcome,
            follows,
            shown_at: Cell::new(None),
            fade_at: Cell::new(None),
            skipped: false,
            too_small: Cell::new(false),
        }
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
        self.next_frame_in_at(self.welcome.started.elapsed(), area)
    }

    fn next_frame_in_at(&self, elapsed: Duration, area: Rect) -> Option<Duration> {
        let (welcome, _) = self.split(area);
        let next = self
            .welcome
            .onigiri
            .borrow()
            .next_frame_in(elapsed, welcome);
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
        let mut welcome = self.welcome.onigiri.borrow_mut();
        let shown_at = self.shown_at.get().unwrap_or(elapsed);
        self.shown_at.set(Some(shown_at));
        if !self.skipped {
            let landed = welcome.landed_at(welcome_area);
            self.fade_at.set(if self.follows {
                Some(landed.map_or(shown_at, |l| l.max(shown_at)))
            } else {
                landed
            });
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

impl<F: WelcomeForm> KeyboardHandler for WelcomeStep<F> {
    fn handle_key_event(&mut self, key_event: KeyEvent) {
        self.handle_key_at(self.welcome.started.elapsed(), key_event);
    }
}

impl<F: WelcomeForm> StepStateProvider for WelcomeStep<F> {
    fn get_step_state(&self) -> StepState {
        self.form.get_step_state()
    }
}

impl<F: WelcomeForm> WidgetRef for WelcomeStep<F> {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        self.render_at(self.welcome.started.elapsed(), area, buf);
    }
}

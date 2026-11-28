//! The first-launch screen: the onigiri welcome above the setup form.

use std::cell::RefCell;
use std::time::Duration;
use std::time::Instant;

use crossterm::event::KeyEvent;
use nori_harness::AgentKind;
use nori_tui_components::onigiri::Caption;
use nori_tui_components::onigiri::Onigiri;
use nori_tui_components::onigiri::Tier;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::WidgetRef;

use super::KeyboardHandler;
use super::StepState;
use super::StepStateProvider;
use super::setup_form::AgentChoice;
use super::setup_form::SetupChoices;
use super::setup_form::SetupForm;

pub(crate) struct NoriSetupStep {
    form: SetupForm,
    welcome: RefCell<Onigiri>,
    started: Instant,
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
        }
    }

    pub(crate) fn choices(&self) -> Option<SetupChoices> {
        self.form.choices()
    }

    fn split(&self, area: Rect) -> (Rect, Rect) {
        let form_h = self.form.height().min(area.height);
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

    /// When the welcome animation next needs a frame, if it still moves.
    pub(crate) fn next_frame_in(&self, area: Rect) -> Option<Duration> {
        let (welcome, _) = self.split(area);
        self.welcome
            .borrow()
            .next_frame_in(self.started.elapsed(), welcome)
    }
}

impl KeyboardHandler for NoriSetupStep {
    fn handle_key_event(&mut self, key_event: KeyEvent) {
        self.form.handle_key_event(key_event);
    }
}

impl StepStateProvider for NoriSetupStep {
    fn get_step_state(&self) -> StepState {
        self.form.get_step_state()
    }
}

impl WidgetRef for NoriSetupStep {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let (welcome, form) = self.split(area);
        self.welcome
            .borrow_mut()
            .render(self.started.elapsed(), welcome, buf);
        self.form.render_ref(form, buf);
    }
}

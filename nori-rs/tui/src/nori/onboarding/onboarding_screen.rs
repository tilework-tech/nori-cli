//! Nori-specific onboarding screen.
//!
//! This module provides a Nori-branded onboarding flow that includes:
//! - First-launch setup: the onigiri welcome and the setup form (if
//!   `~/.nori/cli/config.toml` doesn't exist)
//! - Directory trust prompt with Nori branding
//!
//! The flow is designed to be used instead of the default Codex onboarding
//! screen when building with Nori branding.

use color_eyre::eyre::Result;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use nori_config::NoriConfig;
use nori_config::resolve_root_git_project_for_trust;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::prelude::Widget;
use ratatui::style::Color;
use ratatui::widgets::Clear;
use ratatui::widgets::WidgetRef;
use std::path::PathBuf;

use super::KeyboardHandler;
use super::StepState;
use super::StepStateProvider;
use super::TrustDirectorySelection;
use crate::tui::FrameRequester;
use crate::tui::Tui;
use crate::tui::TuiEvent;
use crate::version::CODEX_CLI_VERSION;
use nori_tui_components::onigiri::Caption;
use nori_tui_components::onigiri::Tier;
use std::time::Duration;

use super::NoriSetupStep;
use super::NoriTrustDirectoryWidget;
use super::is_first_launch;
use super::mark_first_launch_complete;
use super::setup_form::detect_agents;
use super::setup_form::save;

/// Steps in the Nori onboarding flow.
#[allow(clippy::large_enum_variant)]
enum NoriStep {
    /// First-launch welcome and setup form.
    Setup(NoriSetupStep),
    /// Directory trust prompt with Nori branding.
    TrustDirectory(NoriTrustDirectoryWidget),
}

/// Arguments for creating a Nori onboarding screen.
pub(crate) struct NoriOnboardingScreenArgs {
    /// Whether to show the directory trust screen.
    pub show_trust_screen: bool,
    /// Whether to skip the first-launch welcome (--skip-welcome flag).
    pub skip_welcome: bool,
    /// Whether to skip the trust directory prompt (--skip-trust-directory flag).
    pub skip_trust_directory: bool,
    /// Application configuration.
    pub config: NoriConfig,
}

/// Result of running the Nori onboarding screen.
pub(crate) struct NoriOnboardingResult {
    /// The user's trust decision for the directory.
    pub directory_trust_decision: Option<TrustDirectorySelection>,
    /// Whether the user requested to exit the application.
    pub should_exit: bool,
    /// Whether first-launch setup saved settings to config.toml.
    pub settings_saved: bool,
}

/// Nori-branded onboarding screen.
pub(crate) struct NoriOnboardingScreen {
    request_frame: FrameRequester,
    steps: Vec<NoriStep>,
    is_done: bool,
    should_exit: bool,
    nori_home: PathBuf,
}

impl NoriOnboardingScreen {
    /// Create a new Nori onboarding screen.
    pub(crate) fn new(tui: &mut Tui, args: NoriOnboardingScreenArgs) -> Self {
        let NoriOnboardingScreenArgs {
            show_trust_screen,
            skip_welcome,
            skip_trust_directory,
            config,
        } = args;

        let cwd = config.cwd.clone();
        // Use Nori-specific home directory (~/.nori/cli) from the canonical config source
        let nori_home = config.nori_home.clone();

        let mut steps: Vec<NoriStep> = Vec::new();

        // First-launch setup unless skipped
        if !skip_welcome && is_first_launch(&nori_home) {
            let caption = Caption {
                version: format!("v{CODEX_CLI_VERSION}"),
                cwd: crate::exec_command::relativize_to_home(&cwd)
                    .map(|p| format!("~/{}", p.display()))
                    .unwrap_or_else(|| cwd.display().to_string()),
            };
            steps.push(NoriStep::Setup(NoriSetupStep::new(
                detect_agents(&nori_home, &config),
                None,
                caption,
                Tier::detect(),
                true,
            )));
        }

        // Add directory trust screen if needed (unless --skip-trust-directory is set)
        if show_trust_screen && !skip_trust_directory {
            let is_git_repo = resolve_root_git_project_for_trust(&cwd).is_some();
            steps.push(NoriStep::TrustDirectory(NoriTrustDirectoryWidget::new(
                nori_home.clone(),
                cwd,
                is_git_repo,
                config.tool_output == nori_config::ToolOutput::Full
                    || config.tool_output_from_command_line,
            )));
        }

        Self {
            request_frame: tui.frame_requester(),
            steps,
            is_done: false,
            should_exit: false,
            nori_home,
        }
    }

    /// Get the currently visible steps (completed + in progress).
    fn current_steps(&self) -> Vec<&NoriStep> {
        let mut out: Vec<&NoriStep> = Vec::new();
        for step in self.steps.iter() {
            match step.get_step_state() {
                StepState::Complete => out.push(step),
                StepState::InProgress => {
                    out.push(step);
                    break;
                }
            }
        }
        out
    }

    /// Get mutable references to currently visible steps.
    fn current_steps_mut(&mut self) -> Vec<&mut NoriStep> {
        let mut out: Vec<&mut NoriStep> = Vec::new();
        for step in self.steps.iter_mut() {
            match step.get_step_state() {
                StepState::Complete => out.push(step),
                StepState::InProgress => {
                    out.push(step);
                    break;
                }
            }
        }
        out
    }

    /// Check if the onboarding flow is complete.
    pub(crate) fn is_done(&self) -> bool {
        self.is_done
            || !self
                .steps
                .iter()
                .any(|step| matches!(step.get_step_state(), StepState::InProgress))
    }

    /// Get the user's directory trust decision.
    pub fn directory_trust_decision(&self) -> Option<TrustDirectorySelection> {
        self.steps.iter().find_map(|step| {
            if let NoriStep::TrustDirectory(widget) = step {
                widget.selection
            } else {
                None
            }
        })
    }

    /// The setup answers, once the setup form is submitted.
    fn setup_choices(&self) -> Option<super::setup_form::SetupChoices> {
        self.steps.iter().find_map(|step| match step {
            NoriStep::Setup(setup) => setup.choices(),
            NoriStep::TrustDirectory(_) => None,
        })
    }

    /// When the in-progress step next needs a frame for its animation.
    fn next_frame_in(&self, area: Rect) -> Option<Duration> {
        match self
            .steps
            .iter()
            .find(|step| step.get_step_state() == StepState::InProgress)
        {
            Some(NoriStep::Setup(setup)) => setup.next_frame_in(area),
            Some(NoriStep::TrustDirectory(_)) | None => None,
        }
    }

    /// Check if the user requested to exit.
    pub fn should_exit(&self) -> bool {
        self.should_exit
    }

    /// Mark first launch as complete (creates config file).
    fn mark_first_launch_done(&self) {
        if let Err(e) = mark_first_launch_complete(&self.nori_home) {
            tracing::warn!("Failed to mark first launch complete: {e}");
        }
    }
}

impl KeyboardHandler for NoriOnboardingScreen {
    fn handle_key_event(&mut self, key_event: KeyEvent) {
        match key_event {
            // Handle quit/exit keys
            KeyEvent {
                code: KeyCode::Char('d'),
                modifiers: crossterm::event::KeyModifiers::CONTROL,
                kind: KeyEventKind::Press,
                ..
            }
            | KeyEvent {
                code: KeyCode::Char('c'),
                modifiers: crossterm::event::KeyModifiers::CONTROL,
                kind: KeyEventKind::Press,
                ..
            }
            | KeyEvent {
                code: KeyCode::Char('q'),
                kind: KeyEventKind::Press,
                ..
            } => {
                self.should_exit = true;
                self.is_done = true;
            }
            _ => {
                // Handle key event on current step
                if let Some(active_step) = self.current_steps_mut().into_iter().last() {
                    active_step.handle_key_event(key_event);
                }
            }
        };
        self.request_frame.schedule_frame();
    }
}

impl WidgetRef for &NoriOnboardingScreen {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);

        match self
            .steps
            .iter()
            .find(|step| matches!(step.get_step_state(), StepState::InProgress))
        {
            Some(NoriStep::TrustDirectory(widget)) => {
                widget.render_ref(area, buf);
                return;
            }
            Some(NoriStep::Setup(setup)) => {
                setup.render_ref(area, buf);
                return;
            }
            None => {}
        }

        // Render steps top-to-bottom, measuring each step's height dynamically.
        let mut y = area.y;
        let bottom = area.y.saturating_add(area.height);
        let width = area.width;

        // Helper to scan a temporary buffer and return number of used rows.
        fn used_rows(tmp: &Buffer, width: u16, height: u16) -> u16 {
            if width == 0 || height == 0 {
                return 0;
            }
            let mut last_non_empty: Option<u16> = None;
            for yy in 0..height {
                let mut any = false;
                for xx in 0..width {
                    let cell = &tmp[(xx, yy)];
                    let has_symbol = !cell.symbol().trim().is_empty();
                    let has_style = cell.fg != Color::Reset
                        || cell.bg != Color::Reset
                        || !cell.modifier.is_empty();
                    if has_symbol || has_style {
                        any = true;
                        break;
                    }
                }
                if any {
                    last_non_empty = Some(yy);
                }
            }
            last_non_empty.map(|v| v + 2).unwrap_or(0)
        }

        let current_steps = self.current_steps();

        for step in current_steps.iter() {
            if y >= bottom {
                break;
            }
            let max_h = bottom.saturating_sub(y);
            if max_h == 0 || width == 0 {
                break;
            }
            let scratch_area = Rect::new(0, 0, width, max_h);
            let mut scratch = Buffer::empty(scratch_area);
            step.render_ref(scratch_area, &mut scratch);
            let h = used_rows(&scratch, width, max_h).min(max_h);
            if h > 0 {
                let target = Rect {
                    x: area.x,
                    y,
                    width,
                    height: h,
                };
                Clear.render(target, buf);
                step.render_ref(target, buf);
                y = y.saturating_add(h);
            }
        }
    }
}

impl KeyboardHandler for NoriStep {
    fn handle_key_event(&mut self, key_event: KeyEvent) {
        match self {
            NoriStep::Setup(step) => step.handle_key_event(key_event),
            NoriStep::TrustDirectory(widget) => widget.handle_key_event(key_event),
        }
    }
}

impl StepStateProvider for NoriStep {
    fn get_step_state(&self) -> StepState {
        match self {
            NoriStep::Setup(step) => step.get_step_state(),
            NoriStep::TrustDirectory(w) => w.get_step_state(),
        }
    }
}

impl WidgetRef for NoriStep {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        match self {
            NoriStep::Setup(step) => step.render_ref(area, buf),
            NoriStep::TrustDirectory(widget) => widget.render_ref(area, buf),
        }
    }
}

/// Run the Nori onboarding application.
pub(crate) async fn run_nori_onboarding_app(
    args: NoriOnboardingScreenArgs,
    tui: &mut Tui,
) -> Result<NoriOnboardingResult> {
    use tokio_stream::StreamExt;

    let mut onboarding_screen = NoriOnboardingScreen::new(tui, args);

    let mut area = Rect::default();
    tui.draw(u16::MAX, |frame| {
        area = frame.area();
        frame.render_widget_ref(&onboarding_screen, area);
    })?;
    if let Some(next) = onboarding_screen.next_frame_in(area) {
        tui.frame_requester().schedule_frame_in(next);
    }

    let tui_events = tui.event_stream();
    tokio::pin!(tui_events);

    while !onboarding_screen.is_done() {
        if let Some(event) = tui_events.next().await {
            match event {
                TuiEvent::Key(key_event) => {
                    onboarding_screen.handle_key_event(key_event);
                }
                TuiEvent::Paste(_) => {
                    // Paste not handled in Nori onboarding
                }
                TuiEvent::Draw => {
                    let mut area = Rect::default();
                    let _ = tui.draw(u16::MAX, |frame| {
                        area = frame.area();
                        frame.render_widget_ref(&onboarding_screen, area);
                    });
                    if let Some(next) = onboarding_screen.next_frame_in(area) {
                        tui.frame_requester().schedule_frame_in(next);
                    }
                }
            }
        }
    }

    // Save the setup answers and mark first launch complete when onboarding
    // finishes successfully.
    let mut settings_saved = false;
    if !onboarding_screen.should_exit() {
        if let Some(choices) = onboarding_screen.setup_choices() {
            match save(&onboarding_screen.nori_home, &choices) {
                Ok(()) => settings_saved = true,
                Err(e) => tracing::warn!("Failed to save first-launch setup: {e:#}"),
            }
        }
        onboarding_screen.mark_first_launch_done();
    }

    Ok(NoriOnboardingResult {
        directory_trust_decision: onboarding_screen.directory_trust_decision(),
        should_exit: onboarding_screen.should_exit(),
        settings_saved,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nori_step_implements_required_traits() {
        // This test verifies the NoriStep enum implements all required traits.
        // The compilation of this test is the actual verification.
        let _setup = NoriStep::Setup(NoriSetupStep::new(
            Vec::new(),
            None,
            Caption {
                version: String::new(),
                cwd: String::new(),
            },
            Tier::Ansi16,
            false,
        ));
    }

    #[test]
    fn nori_onboarding_args_only_require_live_fields() -> std::io::Result<()> {
        let _args = NoriOnboardingScreenArgs {
            show_trust_screen: true,
            skip_welcome: false,
            skip_trust_directory: false,
            config: NoriConfig {
                cwd: std::env::temp_dir(),
                ..NoriConfig::default()
            },
        };

        Ok(())
    }
}

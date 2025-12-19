//! Nori-specific onboarding screen.
//!
//! This module provides a Nori-branded onboarding flow that includes:
//! - First-launch welcome screen (if `~/.nori/cli/config.toml` doesn't exist)
//! - Directory trust prompt with Nori branding
//!
//! The flow is designed to be used instead of the default Codex onboarding
//! screen when building with Nori branding.

use std::path::PathBuf;
use std::sync::Arc;

use codex_core::AuthManager;
use codex_core::config::Config;
use codex_core::git_info::get_git_repo_root;
use color_eyre::eyre::Result;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::prelude::Widget;
use ratatui::style::Color;
use ratatui::widgets::Clear;
use ratatui::widgets::WidgetRef;

use crate::LoginStatus;
use crate::onboarding::TrustDirectorySelection;
use crate::onboarding::onboarding_screen::KeyboardHandler;
use crate::onboarding::onboarding_screen::StepState;
use crate::onboarding::onboarding_screen::StepStateProvider;
use crate::tui::FrameRequester;
use crate::tui::Tui;
use crate::tui::TuiEvent;

use super::NoriTrustDirectoryWidget;
use super::NoriWelcomeWidget;
use super::is_first_launch;
use super::mark_first_launch_complete;
use crate::nori::config_adapter::get_nori_home;

/// Steps in the Nori onboarding flow.
#[allow(clippy::large_enum_variant)]
enum NoriStep {
    /// First-launch welcome screen with Nori ASCII banner.
    Welcome(NoriWelcomeWidget),
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
    /// Current login status (unused in Nori but kept for API compatibility).
    #[allow(dead_code)]
    pub login_status: LoginStatus,
    /// Auth manager (unused in Nori but kept for API compatibility).
    #[allow(dead_code)]
    pub auth_manager: Arc<AuthManager>,
    /// Application configuration.
    pub config: Config,
}

/// Result of running the Nori onboarding screen.
pub(crate) struct NoriOnboardingResult {
    /// The user's trust decision for the directory.
    pub directory_trust_decision: Option<TrustDirectorySelection>,
    /// Whether the user requested to exit the application.
    pub should_exit: bool,
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
            login_status: _,
            auth_manager: _,
            config,
        } = args;

        let cwd = config.cwd.clone();
        // Use Nori-specific home directory (~/.nori/cli) from the canonical config source
        let nori_home = get_nori_home().unwrap_or_else(|_| config.codex_home.clone());

        let mut steps: Vec<NoriStep> = Vec::new();

        // Add welcome screen if this is the first launch and not skipped
        if !skip_welcome && is_first_launch(&nori_home) {
            steps.push(NoriStep::Welcome(NoriWelcomeWidget::new()));
        }

        // Add directory trust screen if needed (unless --skip-trust-directory is set)
        if show_trust_screen && !skip_trust_directory {
            let is_git_repo = get_git_repo_root(&cwd).is_some();
            let highlighted = if is_git_repo {
                TrustDirectorySelection::Trust
            } else {
                TrustDirectorySelection::DontTrust
            };

            steps.push(NoriStep::TrustDirectory(NoriTrustDirectoryWidget {
                cwd,
                // TODO: This should use Nori-specific config for trust levels
                // For now we delegate to codex_home
                nori_home: nori_home.clone(),
                is_git_repo,
                selection: None,
                highlighted,
                error: None,
            }));
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
                StepState::Hidden => continue,
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
                StepState::Hidden => continue,
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
            NoriStep::Welcome(widget) => widget.handle_key_event(key_event),
            NoriStep::TrustDirectory(widget) => widget.handle_key_event(key_event),
        }
    }
}

impl StepStateProvider for NoriStep {
    fn get_step_state(&self) -> StepState {
        match self {
            NoriStep::Welcome(w) => w.get_step_state(),
            NoriStep::TrustDirectory(w) => w.get_step_state(),
        }
    }
}

impl WidgetRef for NoriStep {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        match self {
            NoriStep::Welcome(widget) => widget.render_ref(area, buf),
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

    tui.draw(u16::MAX, |frame| {
        frame.render_widget_ref(&onboarding_screen, frame.area());
    })?;

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
                    let _ = tui.draw(u16::MAX, |frame| {
                        frame.render_widget_ref(&onboarding_screen, frame.area());
                    });
                }
            }
        }
    }

    // Mark first launch as complete when onboarding finishes successfully
    if !onboarding_screen.should_exit() {
        onboarding_screen.mark_first_launch_done();
    }

    Ok(NoriOnboardingResult {
        directory_trust_decision: onboarding_screen.directory_trust_decision(),
        should_exit: onboarding_screen.should_exit(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nori_step_implements_required_traits() {
        // This test verifies the NoriStep enum implements all required traits.
        // The compilation of this test is the actual verification.
        let _welcome = NoriStep::Welcome(NoriWelcomeWidget::new());
    }
}

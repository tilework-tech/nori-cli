//! Nori-branded directory trust widget.
//!
//! Displays a prompt asking users whether to trust the current directory,
//! with Nori branding instead of Codex.

use std::path::PathBuf;

// TODO: Replace with Nori-specific config when available.
// Currently delegates to codex_core for trust level persistence.
use codex_core::config::set_project_trust_level;
use codex_core::git_info::resolve_root_git_project_for_trust;
use codex_protocol::config_types::TrustLevel;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::WidgetRef;
use ratatui::widgets::Wrap;

use crate::key_hint;
use crate::onboarding::TrustDirectorySelection;
use crate::onboarding::onboarding_screen::KeyboardHandler;
use crate::onboarding::onboarding_screen::StepState;
use crate::onboarding::onboarding_screen::StepStateProvider;
use crate::render::Insets;
use crate::render::renderable::ColumnRenderable;
use crate::render::renderable::Renderable;
use crate::render::renderable::RenderableExt as _;
use crate::selection_list::selection_option_row;

/// Nori-branded directory trust widget.
pub(crate) struct NoriTrustDirectoryWidget {
    /// Path to Nori home directory for config storage.
    /// TODO: Update to use Nori-specific config path when available.
    pub nori_home: PathBuf,
    /// Current working directory being evaluated.
    pub cwd: PathBuf,
    /// Whether the current directory is a git repository.
    pub is_git_repo: bool,
    /// User's selection, if any.
    pub selection: Option<TrustDirectorySelection>,
    /// Currently highlighted option.
    pub highlighted: TrustDirectorySelection,
    /// Error message to display, if any.
    pub error: Option<String>,
}

impl WidgetRef for &NoriTrustDirectoryWidget {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let mut column = ColumnRenderable::new();

        column.push(Line::from(vec![
            "> ".into(),
            "You are running Nori in ".bold(),
            self.cwd.to_string_lossy().to_string().into(),
        ]));
        column.push("");

        let guidance = if self.is_git_repo {
            "Since this folder is version controlled, you may wish to allow Nori to work in this folder without asking for approval."
        } else {
            "Since this folder is not version controlled, we recommend requiring approval of all edits and commands."
        };

        column.push(
            Paragraph::new(guidance.to_string())
                .wrap(Wrap { trim: true })
                .inset(Insets::tlbr(0, 2, 0, 0)),
        );
        column.push("");

        let mut options: Vec<(&str, TrustDirectorySelection)> = Vec::new();
        if self.is_git_repo {
            options.push((
                "Yes, allow Nori to work in this folder without asking for approval",
                TrustDirectorySelection::Trust,
            ));
            options.push((
                "No, ask me to approve edits and commands",
                TrustDirectorySelection::DontTrust,
            ));
        } else {
            options.push((
                "Allow Nori to work in this folder without asking for approval",
                TrustDirectorySelection::Trust,
            ));
            options.push((
                "Require approval of edits and commands",
                TrustDirectorySelection::DontTrust,
            ));
        }

        for (idx, (text, selection)) in options.iter().enumerate() {
            column.push(selection_option_row(
                idx,
                text.to_string(),
                self.highlighted == *selection,
            ));
        }

        column.push("");

        if let Some(error) = &self.error {
            column.push(
                Paragraph::new(error.to_string())
                    .red()
                    .wrap(Wrap { trim: true })
                    .inset(Insets::tlbr(0, 2, 0, 0)),
            );
            column.push("");
        }

        column.push(
            Line::from(vec![
                "Press ".dim(),
                key_hint::plain(KeyCode::Enter).into(),
                " to continue".dim(),
            ])
            .inset(Insets::tlbr(0, 2, 0, 0)),
        );

        column.render(area, buf);
    }
}

impl KeyboardHandler for NoriTrustDirectoryWidget {
    fn handle_key_event(&mut self, key_event: KeyEvent) {
        if key_event.kind == KeyEventKind::Release {
            return;
        }

        match key_event.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.highlighted = TrustDirectorySelection::Trust;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.highlighted = TrustDirectorySelection::DontTrust;
            }
            KeyCode::Char('1') | KeyCode::Char('y') => self.handle_trust(),
            KeyCode::Char('2') | KeyCode::Char('n') => self.handle_dont_trust(),
            KeyCode::Enter => match self.highlighted {
                TrustDirectorySelection::Trust => self.handle_trust(),
                TrustDirectorySelection::DontTrust => self.handle_dont_trust(),
            },
            _ => {}
        }
    }
}

impl StepStateProvider for NoriTrustDirectoryWidget {
    fn get_step_state(&self) -> StepState {
        match self.selection {
            Some(_) => StepState::Complete,
            None => StepState::InProgress,
        }
    }
}

impl NoriTrustDirectoryWidget {
    fn handle_trust(&mut self) {
        let target =
            resolve_root_git_project_for_trust(&self.cwd).unwrap_or_else(|| self.cwd.clone());

        // TODO: Update to use Nori-specific config when available.
        // Currently delegates to codex_core for trust level persistence.
        if let Err(e) = set_project_trust_level(&self.nori_home, &target, TrustLevel::Trusted) {
            tracing::error!("Failed to set project trusted: {e:?}");
            self.error = Some(format!("Failed to set trust for {}: {e}", target.display()));
        }

        self.selection = Some(TrustDirectorySelection::Trust);
    }

    fn handle_dont_trust(&mut self) {
        self.highlighted = TrustDirectorySelection::DontTrust;
        let target =
            resolve_root_git_project_for_trust(&self.cwd).unwrap_or_else(|| self.cwd.clone());

        // TODO: Update to use Nori-specific config when available.
        // Currently delegates to codex_core for trust level persistence.
        if let Err(e) = set_project_trust_level(&self.nori_home, &target, TrustLevel::Untrusted) {
            tracing::error!("Failed to set project untrusted: {e:?}");
            self.error = Some(format!(
                "Failed to set untrusted for {}: {e}",
                target.display()
            ));
        }

        self.selection = Some(TrustDirectorySelection::DontTrust);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    use tempfile::TempDir;

    fn create_widget(is_git_repo: bool) -> (NoriTrustDirectoryWidget, TempDir) {
        let nori_home = TempDir::new().expect("create temp dir");
        let widget = NoriTrustDirectoryWidget {
            nori_home: nori_home.path().to_path_buf(),
            cwd: PathBuf::from("/workspace/project"),
            is_git_repo,
            selection: None,
            highlighted: if is_git_repo {
                TrustDirectorySelection::Trust
            } else {
                TrustDirectorySelection::DontTrust
            },
            error: None,
        };
        (widget, nori_home)
    }

    fn render_widget(widget: &NoriTrustDirectoryWidget, width: u16, height: u16) -> String {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        widget.render_ref(area, &mut buf);

        let mut lines: Vec<String> = Vec::new();
        for y in 0..height {
            let mut line = String::new();
            for x in 0..width {
                line.push_str(buf[(x, y)].symbol());
            }
            lines.push(line.trim_end().to_string());
        }

        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }

        lines.join("\n")
    }

    #[test]
    fn renders_nori_branding_for_git_repo() {
        let (widget, _tmp) = create_widget(true);
        let output = render_widget(&widget, 80, 15);

        assert!(
            output.contains("You are running Nori in"),
            "Should contain Nori branding"
        );
        assert!(
            output.contains("allow Nori to work"),
            "Should use Nori in options"
        );
        assert!(
            !output.contains("Codex"),
            "Should not contain Codex branding"
        );
    }

    #[test]
    fn renders_nori_branding_for_non_git_repo() {
        let (widget, _tmp) = create_widget(false);
        let output = render_widget(&widget, 80, 15);

        assert!(
            output.contains("You are running Nori in"),
            "Should contain Nori branding"
        );
        assert!(
            output.contains("Allow Nori to work"),
            "Should use Nori in options"
        );
        assert!(
            !output.contains("Codex"),
            "Should not contain Codex branding"
        );
    }

    #[test]
    fn starts_in_progress() {
        let (widget, _tmp) = create_widget(true);
        assert_eq!(widget.get_step_state(), StepState::InProgress);
    }

    #[test]
    fn completes_on_enter_trust() {
        let (mut widget, _tmp) = create_widget(true);
        widget.highlighted = TrustDirectorySelection::Trust;

        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        widget.handle_key_event(enter);

        assert_eq!(widget.get_step_state(), StepState::Complete);
        assert_eq!(widget.selection, Some(TrustDirectorySelection::Trust));
    }

    #[test]
    fn completes_on_enter_dont_trust() {
        let (mut widget, _tmp) = create_widget(true);
        widget.highlighted = TrustDirectorySelection::DontTrust;

        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        widget.handle_key_event(enter);

        assert_eq!(widget.get_step_state(), StepState::Complete);
        assert_eq!(widget.selection, Some(TrustDirectorySelection::DontTrust));
    }

    #[test]
    fn navigates_with_arrow_keys() {
        let (mut widget, _tmp) = create_widget(true);

        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        widget.handle_key_event(down);
        assert_eq!(widget.highlighted, TrustDirectorySelection::DontTrust);

        let up = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
        widget.handle_key_event(up);
        assert_eq!(widget.highlighted, TrustDirectorySelection::Trust);
    }

    #[test]
    fn navigates_with_vim_keys() {
        let (mut widget, _tmp) = create_widget(true);

        let j = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        widget.handle_key_event(j);
        assert_eq!(widget.highlighted, TrustDirectorySelection::DontTrust);

        let k = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE);
        widget.handle_key_event(k);
        assert_eq!(widget.highlighted, TrustDirectorySelection::Trust);
    }

    #[test]
    fn ignores_release_events() {
        let (mut widget, _tmp) = create_widget(true);

        let release = KeyEvent {
            kind: KeyEventKind::Release,
            ..KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
        };
        widget.handle_key_event(release);

        assert_eq!(widget.get_step_state(), StepState::InProgress);
    }

    #[test]
    fn snapshot_git_repo() {
        let (widget, _tmp) = create_widget(true);
        let output = render_widget(&widget, 70, 12);
        insta::assert_snapshot!("nori_trust_git_repo", output);
    }

    #[test]
    fn snapshot_non_git_repo() {
        let (widget, _tmp) = create_widget(false);
        let output = render_widget(&widget, 70, 12);
        insta::assert_snapshot!("nori_trust_non_git_repo", output);
    }
}

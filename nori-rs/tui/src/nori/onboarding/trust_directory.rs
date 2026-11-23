//! Nori-branded directory trust widget.
//!
//! Displays a prompt asking users whether to trust the current directory,
//! with Nori branding instead of Codex.

use std::path::PathBuf;

use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use nori_config::ToolOutput;
use nori_config::TrustLevel;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::StatefulWidget;
use ratatui::widgets::WidgetRef;

use super::KeyboardHandler;
use super::StepState;
use super::StepStateProvider;
use super::TrustDirectorySelection;
use nori_config::NoriConfigEdits;
use nori_config::resolve_root_git_project_for_trust;
use nori_tui_components::KeyHint;
use nori_tui_components::MenuDensity;
use nori_tui_components::MenuItem;
use nori_tui_components::MenuOutcome;
use nori_tui_components::MenuRowPattern;
use nori_tui_components::MenuState;
use nori_tui_components::OverlayMenu;

/// Nori-branded directory trust widget.
pub(crate) struct NoriTrustDirectoryWidget {
    /// Path to Nori home directory for config storage.
    pub nori_home: PathBuf,
    /// Current working directory being evaluated.
    pub cwd: PathBuf,
    /// Whether the current directory is a git repository.
    pub is_git_repo: bool,
    /// User's selection, if any.
    pub selection: Option<TrustDirectorySelection>,
    menu: MenuState<TrustDirectorySelection>,
    /// Whether to ask the tool output question after the trust answer.
    ask_tool_output: bool,
    /// Project saved by the trust answer; the compact opt-in uses the same key.
    trust_target: Option<PathBuf>,
    /// Second, skippable question shown after the trust choice.
    tool_output_menu: Option<MenuState<ToolOutput>>,
    /// Error message to display, if any.
    pub error: Option<String>,
}

impl WidgetRef for &NoriTrustDirectoryWidget {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let guidance = if self.is_git_repo {
            "Since this folder is version controlled, you may wish to allow Nori to work in this folder without asking for approval."
        } else {
            "Since this folder is not version controlled, we recommend requiring approval of all edits and commands."
        };
        let subtitle = |text: &str| self.error.as_deref().unwrap_or(text).to_string();
        let navigation = KeyHint::new("↑↓/j/k", "move");
        if let Some(menu) = &self.tool_output_menu {
            styled_menu(
                "Use compact tool output in this project?".to_string(),
                subtitle(
                    "Compact shows one line per tool call. Ctrl+O opens the full transcript either way.",
                ),
                [
                    navigation,
                    KeyHint::new("enter", "select"),
                    KeyHint::new("esc", "skip"),
                ],
            )
            .render(area, buf, &mut menu.clone());
            return;
        }
        styled_menu(
            format!("You are running Nori in {}", self.cwd.display()),
            subtitle(guidance),
            [
                navigation,
                KeyHint::new("1-2", "select"),
                KeyHint::new("enter", "select"),
            ],
        )
        .render(area, buf, &mut self.menu.clone());
    }
}

fn styled_menu<'a, K>(
    title: String,
    subtitle: String,
    hints: [KeyHint<'a>; 3],
) -> OverlayMenu<'a, K> {
    OverlayMenu::new(title)
        .subtitle(subtitle)
        .theme(crate::style::component_theme())
        .max_width(76)
        .density(MenuDensity::Dense)
        .row_pattern(MenuRowPattern::Zebra)
        .fullscreen_selection_rails(true)
        .key_hints(hints)
}

impl KeyboardHandler for NoriTrustDirectoryWidget {
    fn handle_key_event(&mut self, key_event: KeyEvent) {
        if key_event.kind == KeyEventKind::Release {
            return;
        }

        let Some(action) = crate::overlay_menu::action_from_key_event(key_event) else {
            return;
        };
        if let Some(menu) = &mut self.tool_output_menu {
            match menu.handle(action) {
                MenuOutcome::Activated(tool_output) => {
                    if let Some(target) = &self.trust_target
                        && let Err(e) = NoriConfigEdits::new(&self.nori_home)
                            .set_project_tool_output(target, tool_output)
                            .apply_blocking()
                    {
                        tracing::error!("Failed to save tool_output: {e:?}");
                    }
                    self.tool_output_menu = None;
                }
                MenuOutcome::Cancelled => self.tool_output_menu = None,
                MenuOutcome::Unchanged | MenuOutcome::SelectionChanged(_) => {}
            }
            return;
        }
        if let MenuOutcome::Activated(selection) = self.menu.handle(action) {
            self.save_trust(selection);
        }
    }
}

impl StepStateProvider for NoriTrustDirectoryWidget {
    fn get_step_state(&self) -> StepState {
        if self.selection.is_some() && self.tool_output_menu.is_none() {
            StepState::Complete
        } else {
            StepState::InProgress
        }
    }
}

impl NoriTrustDirectoryWidget {
    pub(crate) fn new(
        nori_home: PathBuf,
        cwd: PathBuf,
        is_git_repo: bool,
        ask_tool_output: bool,
    ) -> Self {
        let items = [
            MenuItem::new(
                TrustDirectorySelection::Trust,
                "Yes, allow Nori to work without approval",
            )
            .description("Allow edits and commands in this folder")
            .mnemonic('y')
            .number_shortcut(1),
            MenuItem::new(
                TrustDirectorySelection::DontTrust,
                "No, require approval for edits and commands",
            )
            .description("Ask before Nori changes files or runs commands")
            .mnemonic('n')
            .number_shortcut(2),
        ];
        let mut menu = crate::overlay_menu::state_from_items(items, "directory trust");
        if !is_git_repo {
            menu.select_key(&TrustDirectorySelection::DontTrust);
        }
        Self {
            nori_home,
            cwd,
            is_git_repo,
            selection: None,
            menu,
            ask_tool_output,
            trust_target: None,
            tool_output_menu: None,
            error: None,
        }
    }

    fn save_trust(&mut self, selection: TrustDirectorySelection) {
        let target =
            resolve_root_git_project_for_trust(&self.cwd).unwrap_or_else(|| self.cwd.clone());
        let level = match selection {
            TrustDirectorySelection::Trust => TrustLevel::Trusted,
            TrustDirectorySelection::DontTrust => TrustLevel::Untrusted,
        };
        if let Err(e) = NoriConfigEdits::new(&self.nori_home)
            .set_project_trust_level(&target, level)
            .apply_blocking()
        {
            tracing::error!("Failed to save trust level: {e:?}");
            self.error = Some(format!(
                "Failed to save trust for {}: {e}",
                target.display()
            ));
        } else {
            self.trust_target = Some(target);
        }
        self.selection = Some(selection);
        if !self.ask_tool_output {
            return;
        }

        let items = [
            MenuItem::new(ToolOutput::Full, "Full tool output")
                .description("Keep every command with all of its output")
                .mnemonic('f'),
            MenuItem::new(ToolOutput::Compact, "Compact tool output")
                .description("One line per tool call with its last output line")
                .mnemonic('c'),
        ];
        self.tool_output_menu = Some(crate::overlay_menu::state_from_items(items, "tool output"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyCode;
    use crossterm::event::KeyModifiers;
    use tempfile::TempDir;

    fn create_widget(is_git_repo: bool) -> (NoriTrustDirectoryWidget, TempDir) {
        let nori_home = TempDir::new().expect("create temp dir");
        let widget = NoriTrustDirectoryWidget::new(
            nori_home.path().to_path_buf(),
            PathBuf::from("/workspace/project"),
            is_git_repo,
            true,
        );
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

        while lines.last().is_some_and(String::is_empty) {
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
            output.contains("allow Nori to work"),
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
    fn hints_do_not_advertise_cancellation() {
        let (widget, _tmp) = create_widget(true);
        let output = render_widget(&widget, 80, 15);

        assert!(!output.contains("esc close"));
        assert!(output.contains("1-2 select"));
    }

    fn press(widget: &mut NoriTrustDirectoryWidget, code: KeyCode) {
        widget.handle_key_event(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn written_config(nori_home: &TempDir) -> nori_config::NoriConfigToml {
        let path = nori_home.path().join("config.toml");
        toml::from_str(&std::fs::read_to_string(path).expect("config written"))
            .expect("valid config")
    }

    fn project_settings(nori_home: &TempDir) -> nori_config::ProjectConfig {
        written_config(nori_home)
            .projects
            .get("/workspace/project")
            .cloned()
            .expect("trust saved for the project")
    }

    fn answer_trust(widget: &mut NoriTrustDirectoryWidget, selection: TrustDirectorySelection) {
        let key = match selection {
            TrustDirectorySelection::Trust => 'y',
            TrustDirectorySelection::DontTrust => 'n',
        };
        press(widget, KeyCode::Char(key));
    }

    #[test]
    fn trust_then_full_choice_is_saved_for_this_project() {
        let (mut widget, tmp) = create_widget(true);
        answer_trust(&mut widget, TrustDirectorySelection::Trust);

        assert_eq!(widget.get_step_state(), StepState::InProgress);
        insta::assert_snapshot!(
            "nori_trust_compact_opt_in_after_trust",
            render_widget(&widget, 70, 16)
        );

        press(&mut widget, KeyCode::Enter);
        assert_eq!(widget.get_step_state(), StepState::Complete);
        assert_eq!(
            project_settings(&tmp),
            nori_config::ProjectConfig {
                trust_level: Some(TrustLevel::Trusted),
                tool_output: Some(ToolOutput::Full),
                tool_output_max_lines: None,
            }
        );
        assert_eq!(written_config(&tmp).tui.tool_output, None);
    }

    #[test]
    fn choosing_compact_saves_it_for_this_project_only() {
        let (mut widget, tmp) = create_widget(true);
        answer_trust(&mut widget, TrustDirectorySelection::Trust);
        press(&mut widget, KeyCode::Char('c'));

        assert_eq!(widget.get_step_state(), StepState::Complete);
        assert_eq!(
            project_settings(&tmp),
            nori_config::ProjectConfig {
                trust_level: Some(TrustLevel::Trusted),
                tool_output: Some(ToolOutput::Compact),
                tool_output_max_lines: None,
            }
        );
        assert_eq!(written_config(&tmp).tui.tool_output, None);
    }

    #[test]
    fn compact_opt_in_can_be_skipped() {
        let (mut widget, tmp) = create_widget(true);
        answer_trust(&mut widget, TrustDirectorySelection::Trust);
        press(&mut widget, KeyCode::Esc);

        assert_eq!(widget.get_step_state(), StepState::Complete);
        assert_eq!(
            project_settings(&tmp),
            nori_config::ProjectConfig {
                trust_level: Some(TrustLevel::Trusted),
                tool_output: None,
                tool_output_max_lines: None,
            }
        );
        assert_eq!(written_config(&tmp).tui.tool_output, None);
    }

    #[test]
    fn requiring_approval_also_offers_compact() {
        let (mut widget, tmp) = create_widget(true);
        answer_trust(&mut widget, TrustDirectorySelection::DontTrust);
        assert_eq!(widget.get_step_state(), StepState::InProgress);
        press(&mut widget, KeyCode::Char('c'));

        assert_eq!(widget.get_step_state(), StepState::Complete);
        assert_eq!(
            project_settings(&tmp),
            nori_config::ProjectConfig {
                trust_level: Some(TrustLevel::Untrusted),
                tool_output: Some(ToolOutput::Compact),
                tool_output_max_lines: None,
            }
        );
    }

    #[test]
    fn navigates_with_arrow_keys() {
        let (mut widget, _tmp) = create_widget(true);

        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        widget.handle_key_event(down);
        assert_eq!(
            widget.menu.selected_item().map(|item| *item.key()),
            Some(TrustDirectorySelection::DontTrust)
        );

        let up = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
        widget.handle_key_event(up);
        assert_eq!(
            widget.menu.selected_item().map(|item| *item.key()),
            Some(TrustDirectorySelection::Trust)
        );
    }

    #[test]
    fn navigates_with_vim_keys() {
        let (mut widget, _tmp) = create_widget(true);

        let j = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        widget.handle_key_event(j);
        assert_eq!(
            widget.menu.selected_item().map(|item| *item.key()),
            Some(TrustDirectorySelection::DontTrust)
        );

        let k = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE);
        widget.handle_key_event(k);
        assert_eq!(
            widget.menu.selected_item().map(|item| *item.key()),
            Some(TrustDirectorySelection::Trust)
        );
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

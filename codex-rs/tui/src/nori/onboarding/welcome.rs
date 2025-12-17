//! Nori-branded welcome widget for first-launch onboarding.
//!
//! Displays the NORI ASCII banner and a welcome message when users
//! launch Nori for the first time.

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::WidgetRef;
use ratatui::widgets::Wrap;

use crate::onboarding::onboarding_screen::KeyboardHandler;
use crate::onboarding::onboarding_screen::StepState;
use crate::onboarding::onboarding_screen::StepStateProvider;

/// ASCII art banner for NORI - reused from session_header.rs
const NORI_BANNER: &[&str] = &[
    r"  _   _  ___  ____  ___ ",
    r" | \ | |/ _ \|  _ \|_ _|",
    r" |  \| | | | | |_) || | ",
    r" | |\  | |_| |  _ < | | ",
    r" |_| \_|\___/|_| \_\___|",
];

/// Nori-branded welcome widget for first-launch experience.
pub(crate) struct NoriWelcomeWidget {
    completed: bool,
}

impl NoriWelcomeWidget {
    pub(crate) fn new() -> Self {
        Self { completed: false }
    }
}

impl WidgetRef for &NoriWelcomeWidget {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let mut lines: Vec<Line> = Vec::new();

        // Empty line for spacing
        lines.push(Line::from(""));

        // ASCII banner in green
        for banner_line in NORI_BANNER {
            lines.push(Line::from((*banner_line).green().bold()));
        }

        // Empty line after banner
        lines.push(Line::from(""));
        lines.push(Line::from(""));

        // Welcome message
        lines.push(Line::from(vec![
            "  Welcome to ".into(),
            "Nori".bold().green(),
            ", your AI coding assistant".into(),
        ]));

        lines.push(Line::from(""));

        // First-launch specific message
        lines.push(Line::from("  Let's get you set up...".dim()));

        lines.push(Line::from(""));
        lines.push(Line::from(""));

        // Continue hint
        lines.push(Line::from(vec![
            "  Press ".dim(),
            "Enter".bold(),
            " to continue".dim(),
        ]));

        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .render(area, buf);
    }
}

impl KeyboardHandler for NoriWelcomeWidget {
    fn handle_key_event(&mut self, key_event: KeyEvent) {
        if key_event.kind == KeyEventKind::Release {
            return;
        }

        if matches!(key_event.code, KeyCode::Enter) {
            self.completed = true;
        }
    }
}

impl StepStateProvider for NoriWelcomeWidget {
    fn get_step_state(&self) -> StepState {
        if self.completed {
            StepState::Complete
        } else {
            StepState::InProgress
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;

    fn render_widget(widget: &NoriWelcomeWidget, width: u16, height: u16) -> String {
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

        // Remove trailing empty lines
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }

        lines.join("\n")
    }

    #[test]
    fn nori_welcome_renders_banner() {
        let widget = NoriWelcomeWidget::new();
        let output = render_widget(&widget, 80, 20);

        // Check for NORI banner patterns - the ASCII art spelling
        assert!(
            output.contains("|  _ \\|_ _|"),
            "Should contain NORI banner top pattern"
        );
        assert!(
            output.contains("|_| \\_|"),
            "Should contain banner bottom pattern"
        );
    }

    #[test]
    fn nori_welcome_renders_welcome_message() {
        let widget = NoriWelcomeWidget::new();
        let output = render_widget(&widget, 80, 20);

        assert!(
            output.contains("Welcome to"),
            "Should contain welcome message"
        );
        assert!(output.contains("Nori"), "Should contain Nori product name");
        assert!(
            output.contains("AI coding assistant"),
            "Should contain description"
        );
    }

    #[test]
    fn nori_welcome_renders_continue_hint() {
        let widget = NoriWelcomeWidget::new();
        let output = render_widget(&widget, 80, 20);

        assert!(output.contains("Press"), "Should contain press instruction");
        assert!(output.contains("Enter"), "Should contain Enter key");
        assert!(output.contains("continue"), "Should contain continue text");
    }

    #[test]
    fn nori_welcome_starts_in_progress() {
        let widget = NoriWelcomeWidget::new();
        assert_eq!(widget.get_step_state(), StepState::InProgress);
    }

    #[test]
    fn nori_welcome_completes_on_enter() {
        let mut widget = NoriWelcomeWidget::new();

        let enter_event = KeyEvent::new(KeyCode::Enter, crossterm::event::KeyModifiers::NONE);
        widget.handle_key_event(enter_event);

        assert_eq!(widget.get_step_state(), StepState::Complete);
    }

    #[test]
    fn nori_welcome_ignores_release_events() {
        let mut widget = NoriWelcomeWidget::new();

        let release_event = KeyEvent {
            kind: KeyEventKind::Release,
            ..KeyEvent::new(KeyCode::Enter, crossterm::event::KeyModifiers::NONE)
        };
        widget.handle_key_event(release_event);

        assert_eq!(widget.get_step_state(), StepState::InProgress);
    }

    #[test]
    fn nori_welcome_snapshot() {
        let widget = NoriWelcomeWidget::new();
        let output = render_widget(&widget, 60, 16);
        insta::assert_snapshot!(output);
    }
}

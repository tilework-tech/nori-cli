//! # Footer Component
//!
//! A configurable footer widget for displaying keyboard shortcuts, hints, and context information.
//!
//! The footer supports multiple display modes and can be customized with different indentation,
//! styling, and content formatting.

use crate::KeyBinding;
use crate::key_hint;
use crate::wrapping::prefix_lines;
use crossterm::event::KeyCode;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::prelude::*;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

/// Display modes for the footer component.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FooterMode {
    /// Shows a compact summary with context indicator and shortcuts hint.
    ShortcutSummary,
    /// Shows a detailed overlay with all available shortcuts.
    ShortcutOverlay,
    /// Shows a custom message.
    CustomMessage,
    /// Shows only the context indicator.
    ContextOnly,
}

/// A keyboard shortcut entry for the shortcut overlay.
#[derive(Clone, Debug)]
pub struct ShortcutEntry {
    /// The key binding for this shortcut.
    pub key: KeyBinding,
    /// The description text for this shortcut.
    pub description: String,
}

/// Configuration for the footer component.
pub struct FooterConfig {
    /// Number of columns to indent the footer content.
    pub indent_cols: usize,
    /// Custom text for the shortcuts hint (e.g., "? for shortcuts").
    pub shortcuts_hint_text: Option<String>,
    /// Custom message for CustomMessage mode.
    pub custom_message: Option<String>,
    /// List of shortcuts to display in ShortcutOverlay mode.
    pub shortcuts: Vec<ShortcutEntry>,
    /// Function to format context percentage display.
    pub context_format_fn: Option<Box<dyn Fn(Option<i64>) -> String>>,
    /// Style for the footer text.
    pub style: Style,
}

impl std::fmt::Debug for FooterConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FooterConfig")
            .field("indent_cols", &self.indent_cols)
            .field("shortcuts_hint_text", &self.shortcuts_hint_text)
            .field("custom_message", &self.custom_message)
            .field("shortcuts", &self.shortcuts)
            .field(
                "context_format_fn",
                &self.context_format_fn.as_ref().map(|_| "<function>"),
            )
            .field("style", &self.style)
            .finish()
    }
}

impl Default for FooterConfig {
    fn default() -> Self {
        Self {
            indent_cols: 2,
            shortcuts_hint_text: Some(" for shortcuts".to_string()),
            custom_message: None,
            shortcuts: Vec::new(),
            context_format_fn: None,
            style: Style::default(),
        }
    }
}

/// Calculate the height required for the footer in the given mode.
pub fn footer_height(config: &FooterConfig, mode: FooterMode, context_percent: Option<i64>) -> u16 {
    footer_lines(config, mode, context_percent).len() as u16
}

/// Render the footer to the given area.
pub fn render_footer(
    area: Rect,
    buf: &mut Buffer,
    config: &FooterConfig,
    mode: FooterMode,
    context_percent: Option<i64>,
) {
    Paragraph::new(prefix_lines(
        footer_lines(config, mode, context_percent),
        " ".repeat(config.indent_cols).into(),
        " ".repeat(config.indent_cols).into(),
    ))
    .style(config.style)
    .render(area, buf);
}

fn footer_lines(
    config: &FooterConfig,
    mode: FooterMode,
    context_percent: Option<i64>,
) -> Vec<Line<'static>> {
    match mode {
        FooterMode::ShortcutSummary => {
            let mut line = context_window_line(config, context_percent);
            if let Some(hint) = &config.shortcuts_hint_text {
                line.push_span(" · ");
                line.extend(vec![
                    key_hint::plain(KeyCode::Char('?')).into(),
                    format!(" {hint}").dim(),
                ]);
            }
            vec![line]
        }
        FooterMode::ShortcutOverlay => build_shortcut_overlay(&config.shortcuts),
        FooterMode::CustomMessage => {
            if let Some(message) = &config.custom_message {
                vec![Line::from(vec![message.clone().dim()])]
            } else {
                vec![Line::from("Custom message".dim())]
            }
        }
        FooterMode::ContextOnly => vec![context_window_line(config, context_percent)],
    }
}

fn context_window_line(config: &FooterConfig, percent: Option<i64>) -> Line<'static> {
    let percent = percent.unwrap_or(100).clamp(0, 100);
    let text = if let Some(format_fn) = &config.context_format_fn {
        format_fn(Some(percent))
    } else {
        format!("{percent}% context left")
    };
    Line::from(vec![text.dim()])
}

fn build_shortcut_overlay(shortcuts: &[ShortcutEntry]) -> Vec<Line<'static>> {
    if shortcuts.is_empty() {
        return vec![Line::from("No shortcuts available".dim())];
    }

    // Simple single-column layout for now
    shortcuts
        .iter()
        .map(|shortcut| {
            Line::from(vec![
                shortcut.key.into(),
                format!(" {}", shortcut.description).into(),
            ])
            .dim()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn snapshot_footer(
        name: &str,
        config: &FooterConfig,
        mode: FooterMode,
        context_percent: Option<i64>,
    ) {
        let height = footer_height(config, mode, context_percent).max(1);
        let mut terminal = Terminal::new(TestBackend::new(80, height)).unwrap();
        terminal
            .draw(|f| {
                let area = Rect::new(0, 0, f.area().width, height);
                render_footer(area, f.buffer_mut(), config, mode, context_percent);
            })
            .unwrap();
        assert_snapshot!(name, terminal.backend());
    }

    #[test]
    fn footer_context_only() {
        let config = FooterConfig::default();
        snapshot_footer(
            "footer_context_only",
            &config,
            FooterMode::ContextOnly,
            Some(75),
        );
    }

    #[test]
    fn footer_shortcut_summary() {
        let config = FooterConfig::default();
        snapshot_footer(
            "footer_shortcut_summary",
            &config,
            FooterMode::ShortcutSummary,
            Some(75),
        );
    }

    #[test]
    fn footer_shortcut_overlay() {
        let mut config = FooterConfig::default();
        config.shortcuts = vec![
            ShortcutEntry {
                key: key_hint::plain(KeyCode::Char('/')),
                description: "for commands".to_string(),
            },
            ShortcutEntry {
                key: key_hint::plain(KeyCode::Char('@')),
                description: "for file paths".to_string(),
            },
            ShortcutEntry {
                key: key_hint::ctrl(KeyCode::Char('c')),
                description: "to exit".to_string(),
            },
        ];
        snapshot_footer(
            "footer_shortcut_overlay",
            &config,
            FooterMode::ShortcutOverlay,
            Some(75),
        );
    }

    #[test]
    fn footer_custom_message() {
        let mut config = FooterConfig::default();
        config.custom_message = Some("Processing your request...".to_string());
        snapshot_footer(
            "footer_custom_message",
            &config,
            FooterMode::CustomMessage,
            Some(75),
        );
    }

    #[test]
    fn footer_height_calculation() {
        let config = FooterConfig::default();
        assert_eq!(footer_height(&config, FooterMode::ContextOnly, None), 1);
        assert_eq!(footer_height(&config, FooterMode::ShortcutSummary, None), 1);

        let mut config_with_shortcuts = FooterConfig::default();
        config_with_shortcuts.shortcuts = vec![
            ShortcutEntry {
                key: key_hint::plain(KeyCode::Char('a')),
                description: "test".to_string(),
            },
            ShortcutEntry {
                key: key_hint::plain(KeyCode::Char('b')),
                description: "test".to_string(),
            },
        ];
        assert_eq!(
            footer_height(&config_with_shortcuts, FooterMode::ShortcutOverlay, None),
            2
        );
    }
}

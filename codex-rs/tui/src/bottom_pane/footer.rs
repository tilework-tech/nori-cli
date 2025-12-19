use crate::key_hint;
use crate::key_hint::KeyBinding;
use crate::render::line_utils::prefix_lines;
use crate::ui_consts::FOOTER_INDENT_COLS;
use crossterm::event::KeyCode;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

#[derive(Clone, Debug)]
pub(crate) struct FooterProps {
    pub(crate) mode: FooterMode,
    pub(crate) esc_backtrack_hint: bool,
    pub(crate) use_shift_enter_hint: bool,
    pub(crate) is_task_running: bool,
    pub(crate) _context_window_percent: Option<i64>,
    pub(crate) git_branch: Option<String>,
    pub(crate) nori_profile: Option<String>,
    pub(crate) nori_version: Option<String>,
    pub(crate) git_lines_added: Option<i32>,
    pub(crate) git_lines_removed: Option<i32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FooterMode {
    CtrlCReminder,
    ShortcutSummary,
    ShortcutOverlay,
    EscHint,
    ContextOnly,
}

pub(crate) fn toggle_shortcut_mode(current: FooterMode, ctrl_c_hint: bool) -> FooterMode {
    if ctrl_c_hint && matches!(current, FooterMode::CtrlCReminder) {
        return current;
    }

    match current {
        FooterMode::ShortcutOverlay | FooterMode::CtrlCReminder => FooterMode::ShortcutSummary,
        _ => FooterMode::ShortcutOverlay,
    }
}

pub(crate) fn esc_hint_mode(current: FooterMode, is_task_running: bool) -> FooterMode {
    if is_task_running {
        current
    } else {
        FooterMode::EscHint
    }
}

pub(crate) fn reset_mode_after_activity(current: FooterMode) -> FooterMode {
    match current {
        FooterMode::EscHint
        | FooterMode::ShortcutOverlay
        | FooterMode::CtrlCReminder
        | FooterMode::ContextOnly => FooterMode::ShortcutSummary,
        other => other,
    }
}

pub(crate) fn footer_height(props: &FooterProps) -> u16 {
    footer_lines(props).len() as u16
}

pub(crate) fn render_footer(area: Rect, buf: &mut Buffer, props: &FooterProps) {
    Paragraph::new(prefix_lines(
        footer_lines(props),
        " ".repeat(FOOTER_INDENT_COLS).into(),
        " ".repeat(FOOTER_INDENT_COLS).into(),
    ))
    .render(area, buf);
}

fn footer_lines(props: &FooterProps) -> Vec<Line<'static>> {
    // Show the context indicator on the left, appended after the primary hint
    // (e.g., "? for shortcuts"). Keep it visible even when typing (i.e., when
    // the shortcut hint is hidden). Hide it only for the multi-line
    // ShortcutOverlay.
    match props.mode {
        FooterMode::CtrlCReminder => vec![ctrl_c_reminder_line(CtrlCReminderState {
            is_task_running: props.is_task_running,
        })],
        FooterMode::ShortcutSummary => {
            let mut line = build_footer_line(props);
            // Only add separator if there's already content
            if !line.spans.is_empty() {
                line.push_span(" · ".dim());
            }
            line.extend(vec![
                key_hint::plain(KeyCode::Char('?')).into(),
                " for shortcuts".dim(),
            ]);
            vec![line]
        }
        FooterMode::ShortcutOverlay => shortcut_overlay_lines(ShortcutsState {
            use_shift_enter_hint: props.use_shift_enter_hint,
            esc_backtrack_hint: props.esc_backtrack_hint,
        }),
        FooterMode::EscHint => vec![esc_hint_line(props.esc_backtrack_hint)],
        FooterMode::ContextOnly => vec![build_footer_line(props)],
    }
}

#[derive(Clone, Copy, Debug)]
struct CtrlCReminderState {
    is_task_running: bool,
}

#[derive(Clone, Copy, Debug)]
struct ShortcutsState {
    use_shift_enter_hint: bool,
    esc_backtrack_hint: bool,
}

fn ctrl_c_reminder_line(state: CtrlCReminderState) -> Line<'static> {
    let action = if state.is_task_running {
        "interrupt"
    } else {
        "quit"
    };
    Line::from(vec![
        key_hint::ctrl(KeyCode::Char('c')).into(),
        format!(" again to {action}").into(),
    ])
    .dim()
}

fn esc_hint_line(esc_backtrack_hint: bool) -> Line<'static> {
    let esc = key_hint::plain(KeyCode::Esc);
    if esc_backtrack_hint {
        Line::from(vec![esc.into(), " again to edit previous message".into()]).dim()
    } else {
        Line::from(vec![
            esc.into(),
            " ".into(),
            esc.into(),
            " to edit previous message".into(),
        ])
        .dim()
    }
}

fn shortcut_overlay_lines(state: ShortcutsState) -> Vec<Line<'static>> {
    let mut commands = Line::from("");
    let mut newline = Line::from("");
    let mut file_paths = Line::from("");
    let mut paste_image = Line::from("");
    let mut edit_previous = Line::from("");
    let mut quit = Line::from("");
    let mut show_transcript = Line::from("");

    for descriptor in SHORTCUTS {
        if let Some(text) = descriptor.overlay_entry(state) {
            match descriptor.id {
                ShortcutId::Commands => commands = text,
                ShortcutId::InsertNewline => newline = text,
                ShortcutId::FilePaths => file_paths = text,
                ShortcutId::PasteImage => paste_image = text,
                ShortcutId::EditPrevious => edit_previous = text,
                ShortcutId::Quit => quit = text,
                ShortcutId::ShowTranscript => show_transcript = text,
            }
        }
    }

    let ordered = vec![
        commands,
        newline,
        file_paths,
        paste_image,
        edit_previous,
        quit,
        Line::from(""),
        show_transcript,
    ];

    build_columns(ordered)
}

fn build_columns(entries: Vec<Line<'static>>) -> Vec<Line<'static>> {
    if entries.is_empty() {
        return Vec::new();
    }

    const COLUMNS: usize = 2;
    const COLUMN_PADDING: [usize; COLUMNS] = [4, 4];
    const COLUMN_GAP: usize = 4;

    let rows = entries.len().div_ceil(COLUMNS);
    let target_len = rows * COLUMNS;
    let mut entries = entries;
    if entries.len() < target_len {
        entries.extend(std::iter::repeat_n(
            Line::from(""),
            target_len - entries.len(),
        ));
    }

    let mut column_widths = [0usize; COLUMNS];

    for (idx, entry) in entries.iter().enumerate() {
        let column = idx % COLUMNS;
        column_widths[column] = column_widths[column].max(entry.width());
    }

    for (idx, width) in column_widths.iter_mut().enumerate() {
        *width += COLUMN_PADDING[idx];
    }

    entries
        .chunks(COLUMNS)
        .map(|chunk| {
            let mut line = Line::from("");
            for (col, entry) in chunk.iter().enumerate() {
                line.extend(entry.spans.clone());
                if col < COLUMNS - 1 {
                    let target_width = column_widths[col];
                    let padding = target_width.saturating_sub(entry.width()) + COLUMN_GAP;
                    line.push_span(Span::from(" ".repeat(padding)));
                }
            }
            line.dim()
        })
        .collect()
}

fn build_footer_line(props: &FooterProps) -> Line<'static> {
    let mut spans = Vec::new();

    // Add git branch if available: "⎇ branch-name" (yellow)
    if let Some(branch) = &props.git_branch {
        #[allow(clippy::disallowed_methods)]
        spans.push(Span::from("⎇ ").yellow());
        #[allow(clippy::disallowed_methods)]
        spans.push(Span::from(branch.clone()).yellow());
        spans.push(Span::from(" · ").dim());
    }

    // Add nori profile if available: "Profile: name" (cyan)
    if let Some(profile) = &props.nori_profile {
        spans.push(Span::from("Profile: ").cyan());
        spans.push(Span::from(profile.clone()).cyan());
        spans.push(Span::from(" · ").dim());
    }

    // Add nori version if available: "Nori v19.1.1" (green)
    if let Some(version) = &props.nori_version {
        spans.push(Span::from("Nori v").green());
        spans.push(Span::from(version.clone()).green());
        spans.push(Span::from(" · ").dim());
    }

    // Add git stats if available: "+10 -3" (green for added, red for removed)
    if let (Some(added), Some(removed)) = (props.git_lines_added, props.git_lines_removed) {
        if added > 0 || removed > 0 {
            spans.push(Span::from(format!("+{added}")).green());
            spans.push(Span::from(" ").dim());
            spans.push(Span::from(format!("-{removed}")).red());
            // Don't add separator after stats - the caller will add "? for shortcuts"
        }
    } else if !spans.is_empty() {
        // Remove trailing separator if no stats were added but we have other content
        spans.pop();
    }

    Line::from(spans)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ShortcutId {
    Commands,
    InsertNewline,
    FilePaths,
    PasteImage,
    EditPrevious,
    Quit,
    ShowTranscript,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ShortcutBinding {
    key: KeyBinding,
    condition: DisplayCondition,
}

impl ShortcutBinding {
    fn matches(&self, state: ShortcutsState) -> bool {
        self.condition.matches(state)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DisplayCondition {
    Always,
    WhenShiftEnterHint,
    WhenNotShiftEnterHint,
}

impl DisplayCondition {
    fn matches(self, state: ShortcutsState) -> bool {
        match self {
            DisplayCondition::Always => true,
            DisplayCondition::WhenShiftEnterHint => state.use_shift_enter_hint,
            DisplayCondition::WhenNotShiftEnterHint => !state.use_shift_enter_hint,
        }
    }
}

struct ShortcutDescriptor {
    id: ShortcutId,
    bindings: &'static [ShortcutBinding],
    prefix: &'static str,
    label: &'static str,
}

impl ShortcutDescriptor {
    fn binding_for(&self, state: ShortcutsState) -> Option<&'static ShortcutBinding> {
        self.bindings.iter().find(|binding| binding.matches(state))
    }

    fn overlay_entry(&self, state: ShortcutsState) -> Option<Line<'static>> {
        let binding = self.binding_for(state)?;
        let mut line = Line::from(vec![self.prefix.into(), binding.key.into()]);
        match self.id {
            ShortcutId::EditPrevious => {
                if state.esc_backtrack_hint {
                    line.push_span(" again to edit previous message");
                } else {
                    line.extend(vec![
                        " ".into(),
                        key_hint::plain(KeyCode::Esc).into(),
                        " to edit previous message".into(),
                    ]);
                }
            }
            _ => line.push_span(self.label),
        };
        Some(line)
    }
}

const SHORTCUTS: &[ShortcutDescriptor] = &[
    ShortcutDescriptor {
        id: ShortcutId::Commands,
        bindings: &[ShortcutBinding {
            key: key_hint::plain(KeyCode::Char('/')),
            condition: DisplayCondition::Always,
        }],
        prefix: "",
        label: " for commands",
    },
    ShortcutDescriptor {
        id: ShortcutId::InsertNewline,
        bindings: &[
            ShortcutBinding {
                key: key_hint::shift(KeyCode::Enter),
                condition: DisplayCondition::WhenShiftEnterHint,
            },
            ShortcutBinding {
                key: key_hint::ctrl(KeyCode::Char('j')),
                condition: DisplayCondition::WhenNotShiftEnterHint,
            },
        ],
        prefix: "",
        label: " for newline",
    },
    ShortcutDescriptor {
        id: ShortcutId::FilePaths,
        bindings: &[ShortcutBinding {
            key: key_hint::plain(KeyCode::Char('@')),
            condition: DisplayCondition::Always,
        }],
        prefix: "",
        label: " for file paths",
    },
    ShortcutDescriptor {
        id: ShortcutId::PasteImage,
        bindings: &[ShortcutBinding {
            key: key_hint::ctrl(KeyCode::Char('v')),
            condition: DisplayCondition::Always,
        }],
        prefix: "",
        label: " to paste images",
    },
    ShortcutDescriptor {
        id: ShortcutId::EditPrevious,
        bindings: &[ShortcutBinding {
            key: key_hint::plain(KeyCode::Esc),
            condition: DisplayCondition::Always,
        }],
        prefix: "",
        label: "",
    },
    ShortcutDescriptor {
        id: ShortcutId::Quit,
        bindings: &[ShortcutBinding {
            key: key_hint::ctrl(KeyCode::Char('c')),
            condition: DisplayCondition::Always,
        }],
        prefix: "",
        label: " to exit",
    },
    ShortcutDescriptor {
        id: ShortcutId::ShowTranscript,
        bindings: &[ShortcutBinding {
            key: key_hint::ctrl(KeyCode::Char('t')),
            condition: DisplayCondition::Always,
        }],
        prefix: "",
        label: " to view transcript",
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn snapshot_footer(name: &str, props: FooterProps) {
        let height = footer_height(&props).max(1);
        let mut terminal = Terminal::new(TestBackend::new(80, height)).unwrap();
        terminal
            .draw(|f| {
                let area = Rect::new(0, 0, f.area().width, height);
                render_footer(area, f.buffer_mut(), &props);
            })
            .unwrap();
        assert_snapshot!(name, terminal.backend());
    }

    #[test]
    fn footer_snapshots() {
        snapshot_footer(
            "footer_shortcuts_default",
            FooterProps {
                mode: FooterMode::ShortcutSummary,
                esc_backtrack_hint: false,
                use_shift_enter_hint: false,
                is_task_running: false,
                _context_window_percent: None,
                git_branch: None,
                nori_profile: None,
                nori_version: None,
                git_lines_added: None,
                git_lines_removed: None,
            },
        );

        snapshot_footer(
            "footer_shortcuts_shift_and_esc",
            FooterProps {
                mode: FooterMode::ShortcutOverlay,
                esc_backtrack_hint: true,
                use_shift_enter_hint: true,
                is_task_running: false,
                _context_window_percent: None,
                git_branch: None,
                nori_profile: None,
                nori_version: None,
                git_lines_added: None,
                git_lines_removed: None,
            },
        );

        snapshot_footer(
            "footer_ctrl_c_quit_idle",
            FooterProps {
                mode: FooterMode::CtrlCReminder,
                esc_backtrack_hint: false,
                use_shift_enter_hint: false,
                is_task_running: false,
                _context_window_percent: None,
                git_branch: None,
                nori_profile: None,
                nori_version: None,
                git_lines_added: None,
                git_lines_removed: None,
            },
        );

        snapshot_footer(
            "footer_ctrl_c_quit_running",
            FooterProps {
                mode: FooterMode::CtrlCReminder,
                esc_backtrack_hint: false,
                use_shift_enter_hint: false,
                is_task_running: true,
                _context_window_percent: None,
                git_branch: None,
                nori_profile: None,
                nori_version: None,
                git_lines_added: None,
                git_lines_removed: None,
            },
        );

        snapshot_footer(
            "footer_esc_hint_idle",
            FooterProps {
                mode: FooterMode::EscHint,
                esc_backtrack_hint: false,
                use_shift_enter_hint: false,
                is_task_running: false,
                _context_window_percent: None,
                git_branch: None,
                nori_profile: None,
                nori_version: None,
                git_lines_added: None,
                git_lines_removed: None,
            },
        );

        snapshot_footer(
            "footer_esc_hint_primed",
            FooterProps {
                mode: FooterMode::EscHint,
                esc_backtrack_hint: true,
                use_shift_enter_hint: false,
                is_task_running: false,
                _context_window_percent: None,
                git_branch: None,
                nori_profile: None,
                nori_version: None,
                git_lines_added: None,
                git_lines_removed: None,
            },
        );

        snapshot_footer(
            "footer_shortcuts_context_running",
            FooterProps {
                mode: FooterMode::ShortcutSummary,
                esc_backtrack_hint: false,
                use_shift_enter_hint: false,
                is_task_running: true,
                _context_window_percent: Some(72),
                git_branch: None,
                nori_profile: None,
                nori_version: None,
                git_lines_added: None,
                git_lines_removed: None,
            },
        );
    }

    // @current-session
    #[test]
    fn footer_props_can_be_constructed_with_nori_fields() {
        let _props = FooterProps {
            mode: FooterMode::ShortcutSummary,
            esc_backtrack_hint: false,
            use_shift_enter_hint: false,
            is_task_running: false,
            _context_window_percent: Some(100),
            git_branch: Some("main".to_string()),
            nori_profile: Some("clifford".to_string()),
            nori_version: Some("19.1.1".to_string()),
            git_lines_added: Some(10),
            git_lines_removed: Some(3),
        };
        // Test passes if this compiles and constructs without error
    }

    // @current-session
    #[test]
    fn footer_with_nori_info() {
        snapshot_footer(
            "footer_with_full_nori_info",
            FooterProps {
                mode: FooterMode::ShortcutSummary,
                esc_backtrack_hint: false,
                use_shift_enter_hint: false,
                is_task_running: false,
                _context_window_percent: Some(72),
                git_branch: Some("feature/test".to_string()),
                nori_profile: Some("clifford".to_string()),
                nori_version: Some("19.1.1".to_string()),
                git_lines_added: Some(10),
                git_lines_removed: Some(3),
            },
        );
    }

    // @current-session
    #[test]
    fn footer_with_only_git_info() {
        snapshot_footer(
            "footer_with_only_git",
            FooterProps {
                mode: FooterMode::ShortcutSummary,
                esc_backtrack_hint: false,
                use_shift_enter_hint: false,
                is_task_running: false,
                _context_window_percent: Some(100),
                git_branch: Some("main".to_string()),
                nori_profile: None,
                nori_version: None,
                git_lines_added: Some(5),
                git_lines_removed: Some(2),
            },
        );
    }

    // @current-session
    #[test]
    fn footer_with_no_nori_info() {
        snapshot_footer(
            "footer_with_no_nori",
            FooterProps {
                mode: FooterMode::ShortcutSummary,
                esc_backtrack_hint: false,
                use_shift_enter_hint: false,
                is_task_running: false,
                _context_window_percent: Some(85),
                git_branch: None,
                nori_profile: None,
                nori_version: None,
                git_lines_added: None,
                git_lines_removed: None,
            },
        );
    }
}

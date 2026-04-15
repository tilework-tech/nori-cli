use crate::bottom_pane::textarea::VimModeState;
use crate::key_hint;
use crate::key_hint::KeyBinding;
use crate::render::line_utils::prefix_lines;
use crate::system_info::NoriVersionSource;
use crate::ui_consts::FOOTER_INDENT_COLS;
use codex_acp::config::FooterSegment;
use codex_acp::config::FooterSegmentConfig;
use codex_protocol::num_format::format_si_suffix;
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
    pub(crate) vertical_footer: bool,
    /// Context window percentage used (0-100).
    pub(crate) context_window_percent: Option<i64>,
    /// Total tokens in context window (for "Context: 34K (27%)" display).
    pub(crate) context_tokens: Option<i64>,
    pub(crate) git_branch: Option<String>,
    /// The approval mode label to display (e.g., "Read Only", "Agent", "Full Access").
    pub(crate) approval_mode_label: Option<String>,
    pub(crate) active_skillsets: Vec<String>,
    pub(crate) nori_version: Option<String>,
    /// The source of the version detection (affects display label).
    pub(crate) nori_version_source: Option<NoriVersionSource>,
    pub(crate) git_lines_added: Option<i32>,
    pub(crate) git_lines_removed: Option<i32>,
    /// Whether the current directory is a git worktree (not the main repo).
    /// When true, the git branch indicator is shown in orange instead of yellow.
    pub(crate) is_worktree: bool,
    /// Input tokens from the external agent transcript, if available.
    pub(crate) input_tokens: Option<i64>,
    /// Output tokens from the external agent transcript, if available.
    pub(crate) output_tokens: Option<i64>,
    /// Cached tokens from the external agent transcript, if available.
    pub(crate) cached_tokens: Option<i64>,
    /// Vim mode state - only shown when vim mode is enabled.
    pub(crate) vim_mode_state: Option<VimModeState>,
    /// Short summary of the first user prompt for this session.
    pub(crate) prompt_summary: Option<String>,
    /// The worktree directory name (e.g., "good-ash-20260205-204831") when in a worktree.
    pub(crate) worktree_name: Option<String>,
    /// Configuration for which footer segments to show.
    pub(crate) footer_segment_config: FooterSegmentConfig,
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
            let segments = footer_segments(props);
            if props.vertical_footer {
                let mut lines = segments;
                lines.push(shortcuts_hint_line());
                lines
            } else {
                let mut line = join_footer_segments(&segments);
                // Only add separator if there's already content
                if !line.spans.is_empty() {
                    line.push_span(" · ".dim());
                }
                line.extend(shortcuts_hint_line().spans);
                vec![line]
            }
        }
        FooterMode::ShortcutOverlay => shortcut_overlay_lines(ShortcutsState {
            use_shift_enter_hint: props.use_shift_enter_hint,
            esc_backtrack_hint: props.esc_backtrack_hint,
        }),
        FooterMode::EscHint => vec![esc_hint_line(props.esc_backtrack_hint)],
        FooterMode::ContextOnly => {
            let segments = footer_segments(props);
            if props.vertical_footer {
                if segments.is_empty() {
                    vec![Line::from("")]
                } else {
                    segments
                }
            } else {
                vec![build_footer_line(props)]
            }
        }
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
    let mut open_editor = Line::from("");
    let mut quit = Line::from("");
    let mut show_transcript = Line::from("");
    let mut toggle_plan_drawer = Line::from("");

    for descriptor in SHORTCUTS {
        if let Some(text) = descriptor.overlay_entry(state) {
            match descriptor.id {
                ShortcutId::Commands => commands = text,
                ShortcutId::InsertNewline => newline = text,
                ShortcutId::FilePaths => file_paths = text,
                ShortcutId::PasteImage => paste_image = text,
                ShortcutId::EditPrevious => edit_previous = text,
                ShortcutId::OpenEditor => open_editor = text,
                ShortcutId::Quit => quit = text,
                ShortcutId::ShowTranscript => show_transcript = text,
                ShortcutId::TogglePlanDrawer => toggle_plan_drawer = text,
            }
        }
    }

    let ordered = vec![
        commands,
        newline,
        file_paths,
        paste_image,
        edit_previous,
        open_editor,
        quit,
        toggle_plan_drawer,
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
    join_footer_segments(&footer_segments(props))
}

fn shortcuts_hint_line() -> Line<'static> {
    Line::from(vec![
        key_hint::plain(KeyCode::Char('?')).into(),
        " for shortcuts".dim(),
    ])
}

fn footer_segments(props: &FooterProps) -> Vec<Line<'static>> {
    let mut segments = Vec::new();
    let config = &props.footer_segment_config;

    // Add prompt summary if available and enabled: "Task: <summary>" (dim)
    if config.is_enabled(FooterSegment::PromptSummary)
        && let Some(summary) = &props.prompt_summary
    {
        segments.push(Line::from(vec![
            "Task: ".dim(),
            Span::from(summary.clone()).dim(),
        ]));
    }

    // Add vim mode indicator if vim mode is enabled and segment is enabled
    if config.is_enabled(FooterSegment::VimMode)
        && let Some(vim_state) = props.vim_mode_state
    {
        let (label, style_fn): (&str, fn(Span<'static>) -> Span<'static>) = match vim_state {
            VimModeState::Normal => ("NORMAL", |s| s.light_blue().bold()),
            VimModeState::Insert => ("INSERT", |s| s.green()),
        };
        segments.push(Line::from(vec![style_fn(Span::from(label))]));
    }

    // Add git branch if available and enabled: "⎇ branch-name"
    // Yellow for main repo, light red (orange-ish) for worktree
    if config.is_enabled(FooterSegment::GitBranch)
        && let Some(branch) = &props.git_branch
    {
        let line = if props.is_worktree {
            // Light red for worktree (distinguishable from yellow, works with ANSI)
            #[allow(clippy::disallowed_methods)]
            Line::from(vec![
                Span::from("⎇ ").light_red(),
                Span::from(branch.clone()).light_red(),
            ])
        } else {
            // Yellow for main repo
            #[allow(clippy::disallowed_methods)]
            Line::from(vec![
                Span::from("⎇ ").yellow(),
                Span::from(branch.clone()).yellow(),
            ])
        };
        segments.push(line);
    }

    // Add worktree directory name if available and enabled: "Worktree: name" (light red)
    if config.is_enabled(FooterSegment::WorktreeName)
        && let Some(name) = &props.worktree_name
    {
        #[allow(clippy::disallowed_methods)]
        segments.push(Line::from(vec![
            Span::from("Worktree: ").light_red(),
            Span::from(name.clone()).light_red(),
        ]));
    }

    // Add git stats if available and enabled: "+10 -3" (green for added, red for removed)
    if config.is_enabled(FooterSegment::GitStats)
        && let (Some(added), Some(removed)) = (props.git_lines_added, props.git_lines_removed)
        && (added > 0 || removed > 0)
    {
        segments.push(Line::from(vec![
            Span::from(format!("+{added}")).green(),
            Span::from(" ").dim(),
            Span::from(format!("-{removed}")).red(),
        ]));
    }

    // Add context window info if available and enabled: "Context 27% (34K)".
    if config.is_enabled(FooterSegment::Context) {
        let formatted_tokens = props
            .context_tokens
            .filter(|&tokens| tokens > 0)
            .map(format_si_suffix);
        let context_text = match (props.context_window_percent, formatted_tokens) {
            (Some(pct), Some(tokens)) => Some(format!("Context {pct}% ({tokens})")),
            (Some(pct), None) => Some(format!("Context {pct}%")),
            (None, Some(tokens)) => Some(format!("Context {tokens}")),
            (None, None) => None,
        };
        if let Some(context_text) = context_text {
            segments.push(Line::from(context_text));
        }
    }

    // Add approval mode if available and enabled: "Approvals: Agent" (magenta)
    if config.is_enabled(FooterSegment::ApprovalMode)
        && let Some(label) = &props.approval_mode_label
    {
        segments.push(Line::from(vec![
            Span::from("Approvals: ").magenta(),
            Span::from(label.clone()).magenta(),
        ]));
    }

    // Add active skillsets if available and enabled: "Skillset: name" or "Skillsets: a, b" (cyan)
    if config.is_enabled(FooterSegment::NoriProfile) && !props.active_skillsets.is_empty() {
        let label = if props.active_skillsets.len() == 1 {
            "Skillset: "
        } else {
            "Skillsets: "
        };
        segments.push(Line::from(vec![
            Span::from(label).cyan(),
            Span::from(props.active_skillsets.join(", ")).cyan(),
        ]));
    }

    // Add nori version if available and enabled: "Skillsets v19.1.1" or "Profiles v19.1.1" (green)
    if config.is_enabled(FooterSegment::NoriVersion)
        && let Some(version) = &props.nori_version
    {
        let label = props
            .nori_version_source
            .map(NoriVersionSource::label)
            .unwrap_or("Skillsets");
        segments.push(Line::from(vec![
            Span::from(format!("{label} v")).green(),
            Span::from(version.clone()).green(),
        ]));
    }

    // Add token usage if available and enabled: "Tokens: 77K total (32K cached)" (dim/gray)
    // Total = input + output + cached (cached tokens are read from cache, so they
    // count toward total tokens processed but are shown separately as "cached").
    if config.is_enabled(FooterSegment::TokenUsage) {
        let input = props.input_tokens.unwrap_or(0);
        let output = props.output_tokens.unwrap_or(0);
        let cached = props.cached_tokens.unwrap_or(0);
        let total = input.saturating_add(output).saturating_add(cached);
        if total > 0 {
            let total_fmt = format_si_suffix(total);
            let mut spans = vec![
                "Tokens: ".dim(),
                Span::from(format!("{total_fmt} total")).dim(),
            ];

            // Add cached portion if non-zero
            if cached > 0 {
                let cached_fmt = format_si_suffix(cached);
                spans.push(Span::from(format!(" ({cached_fmt} cached)")).dim());
            }

            segments.push(Line::from(spans));
        }
    }

    segments
}

fn join_footer_segments(segments: &[Line<'static>]) -> Line<'static> {
    let mut line = Line::from("");
    for (idx, segment) in segments.iter().enumerate() {
        if idx > 0 {
            line.push_span(" · ".dim());
        }
        line.extend(segment.spans.clone());
    }
    line
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ShortcutId {
    Commands,
    InsertNewline,
    FilePaths,
    PasteImage,
    EditPrevious,
    OpenEditor,
    Quit,
    ShowTranscript,
    TogglePlanDrawer,
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
        id: ShortcutId::OpenEditor,
        bindings: &[ShortcutBinding {
            key: key_hint::ctrl(KeyCode::Char('g')),
            condition: DisplayCondition::Always,
        }],
        prefix: "",
        label: " to open editor",
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
    ShortcutDescriptor {
        id: ShortcutId::TogglePlanDrawer,
        bindings: &[ShortcutBinding {
            key: key_hint::ctrl(KeyCode::Char('o')),
            condition: DisplayCondition::Always,
        }],
        prefix: "",
        label: " to toggle plan drawer",
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;
    use pretty_assertions::assert_eq;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn default_props() -> FooterProps {
        FooterProps {
            mode: FooterMode::ShortcutSummary,
            esc_backtrack_hint: false,
            use_shift_enter_hint: false,
            is_task_running: false,
            vertical_footer: false,
            context_window_percent: None,
            context_tokens: None,
            git_branch: None,
            approval_mode_label: None,
            active_skillsets: Vec::new(),
            nori_version: None,
            nori_version_source: None,
            git_lines_added: None,
            git_lines_removed: None,
            is_worktree: false,
            input_tokens: None,
            output_tokens: None,
            cached_tokens: None,
            vim_mode_state: None,
            prompt_summary: None,
            worktree_name: None,
            footer_segment_config: FooterSegmentConfig::default(),
        }
    }

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

    fn render_footer_text(props: FooterProps) -> String {
        let height = footer_height(&props).max(1);
        let mut terminal = Terminal::new(TestBackend::new(80, height)).unwrap();
        terminal
            .draw(|f| {
                let area = Rect::new(0, 0, f.area().width, height);
                render_footer(area, f.buffer_mut(), &props);
            })
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn footer_snapshots() {
        snapshot_footer("footer_shortcuts_default", default_props());

        snapshot_footer(
            "footer_shortcuts_shift_and_esc",
            FooterProps {
                mode: FooterMode::ShortcutOverlay,
                esc_backtrack_hint: true,
                use_shift_enter_hint: true,
                ..default_props()
            },
        );

        snapshot_footer(
            "footer_ctrl_c_quit_idle",
            FooterProps {
                mode: FooterMode::CtrlCReminder,
                ..default_props()
            },
        );

        snapshot_footer(
            "footer_ctrl_c_quit_running",
            FooterProps {
                mode: FooterMode::CtrlCReminder,
                is_task_running: true,
                ..default_props()
            },
        );

        snapshot_footer(
            "footer_esc_hint_idle",
            FooterProps {
                mode: FooterMode::EscHint,
                ..default_props()
            },
        );

        snapshot_footer(
            "footer_esc_hint_primed",
            FooterProps {
                mode: FooterMode::EscHint,
                esc_backtrack_hint: true,
                ..default_props()
            },
        );

        snapshot_footer(
            "footer_shortcuts_context_running",
            FooterProps {
                is_task_running: true,
                context_window_percent: Some(72),
                ..default_props()
            },
        );
    }

    #[test]
    fn footer_with_nori_info() {
        snapshot_footer(
            "footer_with_full_nori_info",
            FooterProps {
                context_window_percent: Some(72),
                git_branch: Some("feature/test".to_string()),
                active_skillsets: vec!["clifford".to_string()],
                nori_version: Some("19.1.1".to_string()),
                nori_version_source: Some(NoriVersionSource::Skillsets),
                git_lines_added: Some(10),
                git_lines_removed: Some(3),
                ..default_props()
            },
        );
    }

    #[test]
    fn footer_with_vertical_layout() {
        snapshot_footer(
            "footer_shortcuts_vertical",
            FooterProps {
                vertical_footer: true,
                context_window_percent: Some(72),
                git_branch: Some("feature/test".to_string()),
                approval_mode_label: Some("Agent".to_string()),
                active_skillsets: vec!["clifford".to_string()],
                nori_version: Some("19.1.1".to_string()),
                nori_version_source: Some(NoriVersionSource::Skillsets),
                git_lines_added: Some(10),
                git_lines_removed: Some(3),
                ..default_props()
            },
        );
    }

    #[test]
    fn footer_with_only_git_info() {
        snapshot_footer(
            "footer_with_only_git",
            FooterProps {
                context_window_percent: Some(100),
                git_branch: Some("main".to_string()),
                git_lines_added: Some(5),
                git_lines_removed: Some(2),
                ..default_props()
            },
        );
    }

    #[test]
    fn footer_with_no_nori_info() {
        snapshot_footer(
            "footer_with_no_nori",
            FooterProps {
                context_window_percent: Some(85),
                ..default_props()
            },
        );
    }

    #[test]
    fn footer_with_worktree_shows_orange_branch() {
        snapshot_footer(
            "footer_with_worktree_orange",
            FooterProps {
                context_window_percent: Some(72),
                git_branch: Some("feature/worktree-branch".to_string()),
                active_skillsets: vec!["clifford".to_string()],
                nori_version: Some("19.1.1".to_string()),
                nori_version_source: Some(NoriVersionSource::Skillsets),
                git_lines_added: Some(5),
                git_lines_removed: Some(2),
                is_worktree: true,
                ..default_props()
            },
        );
    }

    #[test]
    fn footer_with_approval_mode() {
        snapshot_footer(
            "footer_with_approval_mode_agent",
            FooterProps {
                context_window_percent: Some(72),
                git_branch: Some("feature/test".to_string()),
                approval_mode_label: Some("Agent".to_string()),
                active_skillsets: vec!["clifford".to_string()],
                nori_version: Some("19.1.1".to_string()),
                nori_version_source: Some(NoriVersionSource::Skillsets),
                git_lines_added: Some(10),
                git_lines_removed: Some(3),
                ..default_props()
            },
        );
    }

    #[test]
    fn footer_with_approval_mode_read_only() {
        snapshot_footer(
            "footer_with_approval_mode_read_only",
            FooterProps {
                git_branch: Some("main".to_string()),
                approval_mode_label: Some("Read Only".to_string()),
                ..default_props()
            },
        );
    }

    #[test]
    fn footer_with_approval_mode_full_access() {
        snapshot_footer(
            "footer_with_approval_mode_full_access",
            FooterProps {
                git_branch: Some("main".to_string()),
                approval_mode_label: Some("Full Access".to_string()),
                ..default_props()
            },
        );
    }

    #[test]
    fn footer_with_token_usage() {
        // Test token usage display: "Tokens: 123K total"
        // Total = input (45K) + output (78K) + cached (0) = 123K
        snapshot_footer(
            "footer_with_token_usage",
            FooterProps {
                context_window_percent: Some(72),
                context_tokens: Some(123456),
                git_branch: Some("feature/test".to_string()),
                active_skillsets: vec!["clifford".to_string()],
                nori_version: Some("19.1.1".to_string()),
                nori_version_source: Some(NoriVersionSource::Skillsets),
                git_lines_added: Some(10),
                git_lines_removed: Some(3),
                input_tokens: Some(45000),
                output_tokens: Some(78456),
                cached_tokens: Some(0),
                ..default_props()
            },
        );
    }

    #[test]
    fn footer_with_large_token_usage() {
        // Test large token usage formats with SI suffix (e.g., 1.23M)
        // Total = input (500K) + output (735K) + cached (0) = 1.23M
        snapshot_footer(
            "footer_with_large_token_usage",
            FooterProps {
                nori_version_source: Some(NoriVersionSource::Profiles),
                context_tokens: Some(1_234_567),
                input_tokens: Some(500_000),
                output_tokens: Some(734_567),
                cached_tokens: Some(0),
                ..default_props()
            },
        );
    }

    #[test]
    fn footer_with_zero_token_usage() {
        // Test that zero tokens does not show the segment
        snapshot_footer(
            "footer_with_zero_token_usage",
            FooterProps {
                nori_version_source: Some(NoriVersionSource::Profiles),
                input_tokens: Some(0),
                output_tokens: Some(0),
                cached_tokens: Some(0),
                ..default_props()
            },
        );
    }

    #[test]
    fn footer_with_cached_tokens() {
        // Test display with cached tokens: "Tokens: 155K total (32K cached)"
        // Total = input (45K) + output (78K) + cached (32K) = 155K
        snapshot_footer(
            "footer_with_cached_tokens",
            FooterProps {
                context_window_percent: Some(27),
                context_tokens: Some(34000),
                input_tokens: Some(45000),
                output_tokens: Some(78000),
                cached_tokens: Some(32000),
                ..default_props()
            },
        );
    }

    #[test]
    fn footer_with_context_no_percent() {
        // Test context display without percentage (when context_window_percent is None)
        // Total = input (20K) + output (14K) + cached (0) = 34K
        snapshot_footer(
            "footer_with_context_no_percent",
            FooterProps {
                context_tokens: Some(34000),
                input_tokens: Some(20000),
                output_tokens: Some(14000),
                cached_tokens: Some(0),
                ..default_props()
            },
        );
    }

    #[test]
    fn footer_context_renders_percent_before_tokens() {
        let rendered = render_footer_text(FooterProps {
            context_window_percent: Some(16),
            context_tokens: Some(42_600),
            ..default_props()
        });

        assert_eq!(rendered.trim(), "Context 16% (42.6K) · ? for shortcuts");
    }

    #[test]
    fn footer_with_vim_mode_normal() {
        snapshot_footer(
            "footer_with_vim_mode_normal",
            FooterProps {
                git_branch: Some("main".to_string()),
                vim_mode_state: Some(VimModeState::Normal),
                ..default_props()
            },
        );
    }

    #[test]
    fn footer_with_vim_mode_insert() {
        snapshot_footer(
            "footer_with_vim_mode_insert",
            FooterProps {
                git_branch: Some("main".to_string()),
                vim_mode_state: Some(VimModeState::Insert),
                ..default_props()
            },
        );
    }

    #[test]
    fn footer_with_prompt_summary() {
        snapshot_footer(
            "footer_with_prompt_summary",
            FooterProps {
                prompt_summary: Some("Fix auth bug".to_string()),
                git_branch: Some("main".to_string()),
                ..default_props()
            },
        );
    }

    #[test]
    fn footer_with_worktree_name() {
        snapshot_footer(
            "footer_with_worktree_name",
            FooterProps {
                git_branch: Some("auto/fix-auth-bug-20260205".to_string()),
                is_worktree: true,
                worktree_name: Some("good-ash-20260205-204831".to_string()),
                active_skillsets: vec!["clifford".to_string()],
                ..default_props()
            },
        );
    }

    #[test]
    fn footer_with_worktree_name_disabled() {
        let segment_config = FooterSegmentConfig {
            worktree_name: false,
            ..Default::default()
        };

        snapshot_footer(
            "footer_with_worktree_name_disabled",
            FooterProps {
                git_branch: Some("auto/fix-auth-bug-20260205".to_string()),
                is_worktree: true,
                worktree_name: Some("good-ash-20260205-204831".to_string()),
                active_skillsets: vec!["clifford".to_string()],
                footer_segment_config: segment_config,
                ..default_props()
            },
        );
    }

    #[test]
    fn footer_with_multiple_active_skillsets() {
        snapshot_footer(
            "footer_with_multiple_active_skillsets",
            FooterProps {
                git_branch: Some("auto/my-branch-20260220".to_string()),
                is_worktree: true,
                active_skillsets: vec!["clifford".to_string(), "rust-dev".to_string()],
                ..default_props()
            },
        );
    }

    #[test]
    fn footer_with_single_active_skillset() {
        snapshot_footer(
            "footer_with_single_active_skillset",
            FooterProps {
                git_branch: Some("main".to_string()),
                active_skillsets: vec!["python-ml".to_string()],
                ..default_props()
            },
        );
    }

    #[test]
    fn footer_with_no_active_skillsets() {
        snapshot_footer(
            "footer_with_no_active_skillsets",
            FooterProps {
                git_branch: Some("main".to_string()),
                active_skillsets: Vec::new(),
                ..default_props()
            },
        );
    }

    // ========================================================================
    // Footer Segment Config Tests
    // ========================================================================

    #[test]
    fn footer_with_git_branch_disabled() {
        let segment_config = FooterSegmentConfig {
            git_branch: false,
            ..Default::default()
        };

        snapshot_footer(
            "footer_with_git_branch_disabled",
            FooterProps {
                git_branch: Some("main".to_string()),
                git_lines_added: Some(10),
                git_lines_removed: Some(3),
                footer_segment_config: segment_config,
                ..default_props()
            },
        );
    }

    #[test]
    fn footer_with_multiple_segments_disabled() {
        let segment_config = FooterSegmentConfig {
            git_branch: false,
            git_stats: false,
            token_usage: false,
            ..Default::default()
        };

        snapshot_footer(
            "footer_with_multiple_segments_disabled",
            FooterProps {
                git_branch: Some("main".to_string()),
                git_lines_added: Some(10),
                git_lines_removed: Some(3),
                context_tokens: Some(34000),
                context_window_percent: Some(27),
                input_tokens: Some(20000),
                output_tokens: Some(14000),
                cached_tokens: Some(0),
                approval_mode_label: Some("Agent".to_string()),
                footer_segment_config: segment_config,
                ..default_props()
            },
        );
    }

    #[test]
    fn footer_with_all_segments_disabled() {
        let segment_config = FooterSegmentConfig {
            prompt_summary: false,
            vim_mode: false,
            git_branch: false,
            worktree_name: false,
            git_stats: false,
            context: false,
            approval_mode: false,
            nori_profile: false,
            nori_version: false,
            token_usage: false,
        };

        snapshot_footer(
            "footer_with_all_segments_disabled",
            FooterProps {
                git_branch: Some("main".to_string()),
                git_lines_added: Some(10),
                git_lines_removed: Some(3),
                context_tokens: Some(34000),
                context_window_percent: Some(27),
                approval_mode_label: Some("Agent".to_string()),
                active_skillsets: vec!["clifford".to_string()],
                nori_version: Some("19.1.1".to_string()),
                input_tokens: Some(20000),
                output_tokens: Some(14000),
                is_worktree: true,
                worktree_name: Some("good-ash-20260205-204831".to_string()),
                footer_segment_config: segment_config,
                ..default_props()
            },
        );
    }

    #[test]
    fn footer_vertical_with_segments_disabled() {
        let segment_config = FooterSegmentConfig {
            nori_profile: false,
            nori_version: false,
            ..Default::default()
        };

        snapshot_footer(
            "footer_vertical_with_segments_disabled",
            FooterProps {
                vertical_footer: true,
                git_branch: Some("feature/test".to_string()),
                context_tokens: Some(34000),
                context_window_percent: Some(27),
                approval_mode_label: Some("Agent".to_string()),
                active_skillsets: vec!["clifford".to_string()],
                nori_version: Some("19.1.1".to_string()),
                footer_segment_config: segment_config,
                ..default_props()
            },
        );
    }
}

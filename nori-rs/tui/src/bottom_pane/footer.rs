use crate::bottom_pane::composer_hints::ComposerHints;
#[cfg(test)]
use crate::bottom_pane::composer_hints::HintDraft;
use crate::bottom_pane::textarea::VimModeState;
use crate::key_hint;
use crate::key_hint::KeyBinding;
use crate::system_info::NoriVersionSource;
use crate::ui_consts::FOOTER_INDENT_COLS;
use crate::ui_types::format_si_suffix;
use crossterm::event::KeyCode;
use nori_config::FooterFormatPart;
use nori_config::FooterLayoutConfig;
use nori_config::FooterLayoutItem;
use nori_config::FooterSegment;
use nori_config::FooterSegmentConfig;
use nori_harness::permission_mode::PermissionMode;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Widget;

const MODE_INDICATOR_LABEL_MAX_CHARS: usize = 20;
/// Minimum blank columns between the left and right footer groups before the
/// right group starts shedding segments.
const FOOTER_GROUP_MIN_GAP_COLS: u16 = 2;
/// Session titles are agent-controlled free text and routinely a whole
/// paragraph. The `/status` card keeps the longer form; the footer row has to
/// share one line with git, context, and approvals.
const SESSION_TITLE_MAX_CHARS: usize = 24;

#[derive(Clone, Debug)]
pub(crate) struct FooterProps {
    pub(crate) mode: FooterMode,
    pub(crate) esc_backtrack_hint: bool,
    pub(crate) use_shift_enter_hint: bool,
    pub(crate) is_task_running: bool,
    pub(crate) vertical_footer: bool,
    /// Context window percentage used (0-100).
    pub(crate) context_window_percent: Option<i64>,
    /// Tokens currently used in the context window.
    pub(crate) context_tokens: Option<i64>,
    /// Maximum tokens available in the context window.
    pub(crate) context_window_tokens: Option<i64>,
    pub(crate) git_branch: Option<String>,
    /// The approval mode label to display (e.g., "Read Only", "Agent", "Full Access").
    pub(crate) approval_mode_label: Option<String>,
    pub(crate) active_skillsets: Vec<String>,
    pub(crate) nori_version: Option<String>,
    /// The source of the version detection (affects display label).
    pub(crate) nori_version_source: Option<NoriVersionSource>,
    pub(crate) git_lines_added: Option<i32>,
    pub(crate) git_lines_removed: Option<i32>,
    pub(crate) git_has_untracked: bool,
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
    /// Agent-supplied session title from ACP session-info updates. Already
    /// sanitized and length-bounded by `crate::nori::session_info`.
    pub(crate) session_title: Option<String>,
    /// The worktree directory name (e.g., "good-ash-20260205-204831") when in a worktree.
    pub(crate) worktree_name: Option<String>,
    /// Configuration for which footer segments to show.
    pub(crate) footer_segment_config: FooterSegmentConfig,
    /// Configuration for where footer segments render.
    pub(crate) footer_layout_config: FooterLayoutConfig,
    /// ACP agent mode label to display when the agent exposes a mode option.
    pub(crate) acp_mode_label: Option<String>,
    /// Cloud session identifier (e.g. `nori-fast-kazunoko-aac8`) when attached
    /// through cloud mode. Renders the `CloudSession` segment.
    pub(crate) cloud_session: Option<String>,
    /// This terminal's standing on a joined host. It leads the footer and is
    /// never hidden by the segment configuration.
    pub(crate) host_status: Option<Line<'static>>,
    /// The keys that work in the composer right now, when the composer
    /// has been told what its session allows.
    pub(crate) composer_hints: Option<ComposerHints>,
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
    for (line_index, row) in footer_rows(props).into_iter().enumerate() {
        let Ok(line_offset) = u16::try_from(line_index) else {
            break;
        };
        if line_offset >= area.height {
            break;
        }

        let row_area = Rect {
            x: area.x,
            y: area.y + line_offset,
            width: area.width,
            height: 1,
        };
        render_footer_row(row_area, buf, row);
    }
}

fn footer_lines(props: &FooterProps) -> Vec<Line<'static>> {
    footer_rows(props)
        .into_iter()
        .map(FooterRow::joined)
        .collect()
}

#[derive(Clone, Debug, Default)]
struct FooterRow {
    left: Vec<Line<'static>>,
    /// Which of `left` are composer hints. A narrow row drops these from the
    /// end, whole, before the rest of the left group is cut.
    hints: std::ops::Range<usize>,
    /// Right-aligned segments, kept separate rather than pre-joined so a narrow
    /// terminal can drop trailing ones instead of painting over the left group.
    right: Vec<Line<'static>>,
}

impl FooterRow {
    fn left(left: Line<'static>) -> Self {
        Self {
            left: vec![left],
            hints: 0..0,
            right: Vec::new(),
        }
    }

    /// The left group with only the first `keep` hints.
    fn left_line(&self, keep: usize) -> Line<'static> {
        let kept: Vec<Line<'static>> = self
            .left
            .iter()
            .enumerate()
            .filter(|(idx, _)| !self.hints.contains(idx) || *idx < self.hints.start + keep)
            .map(|(_, line)| line.clone())
            .collect();
        join_footer_segments(&kept)
    }

    fn joined(self) -> Line<'static> {
        let right = join_footer_segments(&self.right);
        let left = self.left_line(self.hints.len());
        if right.spans.is_empty() {
            left
        } else if left.spans.is_empty() {
            right
        } else {
            let mut line = left;
            line.push_span(" · ".dim());
            line.extend(right.spans);
            line
        }
    }
}

fn render_footer_row(area: Rect, buf: &mut Buffer, row: FooterRow) {
    if area.is_empty() {
        return;
    }

    let footer_indent = u16::try_from(FOOTER_INDENT_COLS).unwrap_or(0);
    let left_area = Rect {
        x: area.x + footer_indent,
        y: area.y,
        width: area.width.saturating_sub(footer_indent),
        height: 1,
    };
    // Hints are the first to go: keep as many as still leave room for the
    // whole right group, then let the right group shed as before.
    let right_cols = line_cols(&join_footer_segments(&row.right));
    let left = (0..=row.hints.len())
        .rev()
        .map(|keep| row.left_line(keep))
        .find(|line| {
            let left_cols = line_cols(line);
            let needed = match (left_cols, right_cols) {
                (_, 0) => left_cols,
                (0, _) => right_cols,
                _ => left_cols
                    .saturating_add(FOOTER_GROUP_MIN_GAP_COLS)
                    .saturating_add(right_cols),
            };
            needed <= left_area.width
        })
        .unwrap_or_else(|| row.left_line(0));
    let left_width = line_cols(&left);
    left.render(left_area, buf);

    // The right group shares the row with the left one, so it only gets the
    // columns the indent and the left group leave behind. Shed trailing
    // segments until it fits; the group narrows and then disappears instead of
    // overwriting the left group on a small terminal.
    let mut reserved = footer_indent.saturating_add(left_width);
    if left_width > 0 {
        reserved = reserved.saturating_add(FOOTER_GROUP_MIN_GAP_COLS);
    }
    let Some(right) = fit_footer_segments(&row.right, area.width.saturating_sub(reserved)) else {
        return;
    };

    let right_width = line_cols(&right);
    let right_area = Rect {
        x: area.right().saturating_sub(right_width),
        y: area.y,
        width: right_width,
        height: 1,
    };
    right.render(right_area, buf);
}

fn line_cols(line: &Line<'static>) -> u16 {
    u16::try_from(line.width()).unwrap_or(u16::MAX)
}

/// Join the longest leading run of `segments` that fits in `available` columns.
/// Returns `None` when nothing fits, which hides the group for that frame.
fn fit_footer_segments(segments: &[Line<'static>], available: u16) -> Option<Line<'static>> {
    (1..=segments.len())
        .rev()
        .map(|end| join_footer_segments(&segments[..end]))
        .find(|candidate| !candidate.spans.is_empty() && line_cols(candidate) <= available)
}

fn footer_rows(props: &FooterProps) -> Vec<FooterRow> {
    // Keep footer metadata visible while typing. Hide it only for the
    // multi-line ShortcutOverlay so the expanded shortcut table owns the
    // footer area.
    match props.mode {
        FooterMode::CtrlCReminder => {
            vec![FooterRow::left(ctrl_c_reminder_line(CtrlCReminderState {
                is_task_running: props.is_task_running,
            }))]
        }
        FooterMode::ShortcutSummary => {
            let footer_segments = footer_segment_groups(props);
            if props.vertical_footer {
                vertical_footer_rows(footer_segments)
            } else {
                footer_row_for_segment_groups(footer_segments)
            }
        }
        FooterMode::ShortcutOverlay => shortcut_overlay_lines(ShortcutsState {
            use_shift_enter_hint: props.use_shift_enter_hint,
            esc_backtrack_hint: props.esc_backtrack_hint,
        })
        .into_iter()
        .map(FooterRow::left)
        .collect(),
        FooterMode::EscHint => vec![FooterRow::left(esc_hint_line(props.esc_backtrack_hint))],
        FooterMode::ContextOnly => {
            let footer_segments = footer_segment_groups(props);
            if props.vertical_footer {
                vertical_footer_rows(footer_segments)
            } else {
                footer_row_for_segment_groups(footer_segments)
            }
        }
    }
}

/// One row per segment, with the composer hints sharing the first.
fn vertical_footer_rows(footer_segments: FooterSegmentGroups) -> Vec<FooterRow> {
    let FooterSegmentGroups { left, hints, right } = footer_segments;
    let hint_row = FooterRow {
        left: left[hints.clone()].to_vec(),
        hints: 0..hints.len(),
        right: Vec::new(),
    };
    (!hints.is_empty())
        .then_some(hint_row)
        .into_iter()
        .chain(
            left.into_iter()
                .skip(hints.end)
                .chain(right)
                .map(FooterRow::left),
        )
        .collect()
}

fn footer_row_for_segment_groups(footer_segments: FooterSegmentGroups) -> Vec<FooterRow> {
    let row = FooterRow {
        left: footer_segments.left,
        hints: footer_segments.hints,
        right: footer_segments.right,
    };
    if row.left.is_empty() && row.right.is_empty() {
        Vec::new()
    } else {
        vec![row]
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
    let mut skills = Line::from("");
    let mut shell_command = Line::from("");
    let mut paste_image = Line::from("");
    let mut edit_previous = Line::from("");
    let mut open_editor = Line::from("");
    let mut quit = Line::from("");
    let mut show_transcript = Line::from("");
    let mut toggle_plan_drawer = Line::from("");
    let mut toggle_screen = Line::from("");

    for descriptor in SHORTCUTS {
        if let Some(text) = descriptor.overlay_entry(state) {
            match descriptor.id {
                ShortcutId::Commands => commands = text,
                ShortcutId::InsertNewline => newline = text,
                ShortcutId::FilePaths => file_paths = text,
                ShortcutId::Skills => skills = text,
                ShortcutId::ShellCommand => shell_command = text,
                ShortcutId::PasteImage => paste_image = text,
                ShortcutId::EditPrevious => edit_previous = text,
                ShortcutId::OpenEditor => open_editor = text,
                ShortcutId::Quit => quit = text,
                ShortcutId::ShowTranscript => show_transcript = text,
                ShortcutId::TogglePlanDrawer => toggle_plan_drawer = text,
                ShortcutId::ToggleScreen => toggle_screen = text,
            }
        }
    }

    let ordered = vec![
        commands,
        newline,
        file_paths,
        skills,
        shell_command,
        paste_image,
        edit_previous,
        open_editor,
        quit,
        toggle_plan_drawer,
        toggle_screen,
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

#[derive(Clone, Debug, Default)]
pub(crate) struct TextareaCornerSegments {
    pub(crate) top_left: Line<'static>,
    pub(crate) top_right: Line<'static>,
    pub(crate) bottom_left: Line<'static>,
    pub(crate) bottom_right: Line<'static>,
}

pub(crate) fn textarea_corner_segments(props: &FooterProps) -> TextareaCornerSegments {
    TextareaCornerSegments {
        top_left: join_footer_segments(&segments_for(
            props,
            &props.footer_layout_config.textarea_top_left,
        )),
        top_right: join_footer_segments(&segments_for(
            props,
            &props.footer_layout_config.textarea_top_right,
        )),
        bottom_left: join_footer_segments(&segments_for(
            props,
            &props.footer_layout_config.textarea_bottom_left,
        )),
        bottom_right: join_footer_segments(&segments_for(
            props,
            &props.footer_layout_config.textarea_bottom_right,
        )),
    }
}

#[derive(Clone, Debug, Default)]
struct FooterSegmentGroups {
    left: Vec<Line<'static>>,
    /// Which of `left` are composer hints.
    hints: std::ops::Range<usize>,
    right: Vec<Line<'static>>,
}

fn footer_segment_groups(props: &FooterProps) -> FooterSegmentGroups {
    let layout = &props.footer_layout_config.footer_left;
    // Hints that open the layout sit directly under the prompt, ahead of the
    // host status; anywhere else they are an ordinary segment.
    let (hints, rest) = match layout.split_first() {
        Some((FooterLayoutItem::Builtin(FooterSegment::ComposerHints), rest)) => {
            (composer_hint_items(props), rest)
        }
        _ => (Vec::new(), layout.as_slice()),
    };
    let hint_count = hints.len();
    FooterSegmentGroups {
        left: hints
            .into_iter()
            .chain(props.host_status.iter().cloned())
            .chain(segments_for(props, rest))
            .collect(),
        hints: 0..hint_count,
        right: segments_for(props, &props.footer_layout_config.footer_right),
    }
}

fn composer_hint_items(props: &FooterProps) -> Vec<Line<'static>> {
    if !props
        .footer_segment_config
        .is_enabled(FooterSegment::ComposerHints)
    {
        return Vec::new();
    }
    props
        .composer_hints
        .map(|hints| hints.items(props.use_shift_enter_hint))
        .unwrap_or_default()
}

fn segments_for(props: &FooterProps, segment_order: &[FooterLayoutItem]) -> Vec<Line<'static>> {
    segment_order
        .iter()
        .filter_map(|item| footer_layout_item(props, item))
        .collect()
}

/// A session that asks every time or approves everything shows its mode
/// even in a footer that hides the segment; plain Agent stays quiet.
fn approval_mode_stands_out(props: &FooterProps) -> bool {
    props
        .approval_mode_label
        .as_deref()
        .is_some_and(|label| label != PermissionMode::Agent.name())
}

fn footer_layout_item(props: &FooterProps, item: &FooterLayoutItem) -> Option<Line<'static>> {
    match item {
        FooterLayoutItem::Builtin(segment)
            if props.footer_segment_config.is_enabled(*segment)
                || (*segment == FooterSegment::ApprovalMode && approval_mode_stands_out(props)) =>
        {
            builtin_footer_segment(props, *segment)
        }
        FooterLayoutItem::Builtin(_) => None,
        FooterLayoutItem::Custom(format) => {
            let mut line = Line::from("");
            for part in format.parts() {
                match part {
                    FooterFormatPart::Text(text) => line.push_span(text.clone()),
                    FooterFormatPart::Segment(segment) => {
                        let segment = builtin_footer_segment(props, *segment)?;
                        line.extend(segment.spans);
                    }
                }
            }
            (!line.spans.is_empty()).then_some(line)
        }
    }
}

fn builtin_footer_segment(props: &FooterProps, segment: FooterSegment) -> Option<Line<'static>> {
    match segment {
        FooterSegment::PromptSummary => props
            .prompt_summary
            .as_ref()
            .map(|summary| Line::from(vec!["Task: ".dim(), Span::from(summary.clone()).dim()])),
        FooterSegment::SessionTitle => props.session_title.as_ref().map(|title| {
            let title = truncate_label(title, SESSION_TITLE_MAX_CHARS);
            Line::from(vec!["Title: ".dim(), Span::from(title).dim()])
        }),
        FooterSegment::VimMode => props.vim_mode_state.map(|vim_state| {
            let (label, style_fn): (&str, fn(Span<'static>) -> Span<'static>) = match vim_state {
                VimModeState::Normal => ("NORMAL", |s| s.light_blue().bold()),
                VimModeState::Insert => ("INSERT", |s| s.green()),
            };
            Line::from(vec![style_fn(Span::from(label))])
        }),
        FooterSegment::GitBranch => props.git_branch.as_ref().map(|branch| {
            if props.is_worktree {
                #[allow(clippy::disallowed_methods)]
                Line::from(vec![
                    Span::from("⎇ ").light_red(),
                    Span::from(branch.clone()).light_red(),
                ])
            } else {
                #[allow(clippy::disallowed_methods)]
                Line::from(vec![
                    Span::from("⎇ ").yellow(),
                    Span::from(branch.clone()).yellow(),
                ])
            }
        }),
        FooterSegment::WorktreeName => props.worktree_name.as_ref().map(|name| {
            #[allow(clippy::disallowed_methods)]
            Line::from(vec![
                Span::from("Worktree: ").light_red(),
                Span::from(name.clone()).light_red(),
            ])
        }),
        FooterSegment::GitStats => git_stats_segment(props),
        FooterSegment::Context => context_segment(props),
        FooterSegment::ContextUsedPercent => context_used_percent_segment(props),
        FooterSegment::ContextRemainingPercent => context_remaining_percent_segment(props),
        FooterSegment::ContextUsedTokens => context_used_tokens_segment(props),
        FooterSegment::ContextRemainingTokens => context_remaining_tokens_segment(props),
        FooterSegment::ContextWindowTokens => context_window_tokens_segment(props),
        FooterSegment::ApprovalMode => props.approval_mode_label.as_ref().map(|label| {
            Line::from(vec![
                Span::from("Approvals: ").magenta(),
                Span::from(label.clone()).magenta(),
            ])
        }),
        FooterSegment::Skillset => skillset_segment(props),
        FooterSegment::NoriVersion => props.nori_version.as_ref().map(|version| {
            let label = props
                .nori_version_source
                .map(NoriVersionSource::label)
                .unwrap_or("Skillsets");
            Line::from(vec![
                Span::from(format!("{label} v")).green(),
                Span::from(version.clone()).green(),
            ])
        }),
        FooterSegment::TokenUsage => token_usage_segment(props),
        FooterSegment::ModeIndicator => props.acp_mode_label.as_ref().map(|raw_label| {
            let label = truncate_label(raw_label, MODE_INDICATOR_LABEL_MAX_CHARS);
            let text = format!("[ {label} ]");
            let lower = raw_label.to_lowercase();
            let span = if lower.contains("plan") {
                Span::from(text).green()
            } else if lower.contains("bypass") {
                Span::from(text).red()
            } else if lower.contains("accept")
                || lower.contains("implement")
                || lower.contains("build")
            {
                Span::from(text).cyan()
            } else {
                Span::from(text).dim()
            };
            Line::from(span)
        }),
        FooterSegment::CloudSession => props
            .cloud_session
            .as_ref()
            .map(|id| Line::from(vec![Span::from("☁ ").cyan(), Span::from(id.clone()).cyan()])),
        FooterSegment::ComposerHints => {
            let items = composer_hint_items(props);
            (!items.is_empty()).then(|| join_footer_segments(&items))
        }
    }
}

fn truncate_label(label: &str, max_chars: usize) -> String {
    if label.chars().count() <= max_chars {
        return label.to_string();
    }

    label
        .chars()
        .take(max_chars.saturating_sub(1))
        .chain(std::iter::once('…'))
        .collect()
}

fn git_stats_segment(props: &FooterProps) -> Option<Line<'static>> {
    let mut spans = Vec::new();
    if let (Some(added), Some(removed)) = (props.git_lines_added, props.git_lines_removed)
        && (added > 0 || removed > 0)
    {
        spans.extend([
            Span::from(format!("+{added}")).green(),
            Span::from(" ").dim(),
            Span::from(format!("-{removed}")).red(),
        ]);
    }
    if props.git_has_untracked {
        if !spans.is_empty() {
            spans.push(Span::from(" ").dim());
        }
        spans.push(Span::from("!").red().bold());
    }
    (!spans.is_empty()).then(|| Line::from(spans))
}

fn context_segment(props: &FooterProps) -> Option<Line<'static>> {
    nori_tui_components::session_table::context_text(
        props.context_window_percent,
        props.context_window_tokens,
        props.context_tokens,
    )
    .map(Line::from)
}

fn context_used_percent_segment(props: &FooterProps) -> Option<Line<'static>> {
    props
        .context_window_percent
        .map(clamp_percent)
        .map(|percent| Line::from(format!("{percent}%")))
}

fn context_remaining_percent_segment(props: &FooterProps) -> Option<Line<'static>> {
    props
        .context_window_percent
        .map(clamp_percent)
        .map(|percent| Line::from(format!("{}%", 100 - percent)))
}

fn context_used_tokens_segment(props: &FooterProps) -> Option<Line<'static>> {
    props
        .context_tokens
        .filter(|&tokens| tokens >= 0)
        .map(format_context_tokens)
        .map(Line::from)
}

fn context_remaining_tokens_segment(props: &FooterProps) -> Option<Line<'static>> {
    props
        .context_window_tokens
        .zip(props.context_tokens)
        .filter(|(window, used)| *window > 0 && *used >= 0)
        .map(|(window, used)| window.saturating_sub(used).max(0))
        .map(format_context_tokens)
        .map(Line::from)
}

fn context_window_tokens_segment(props: &FooterProps) -> Option<Line<'static>> {
    props
        .context_window_tokens
        .filter(|&tokens| tokens > 0)
        .map(format_context_tokens)
        .map(Line::from)
}

fn clamp_percent(percent: i64) -> i64 {
    percent.clamp(0, 100)
}

fn format_context_tokens(tokens: i64) -> String {
    nori_tui_components::session_table::compact_tokens(tokens)
}

fn skillset_segment(props: &FooterProps) -> Option<Line<'static>> {
    if props.active_skillsets.is_empty() {
        return None;
    }
    let label = if props.active_skillsets.len() == 1 {
        "Skillset: "
    } else {
        "Skillsets: "
    };
    Some(Line::from(vec![
        Span::from(label).cyan(),
        Span::from(props.active_skillsets.join(", ")).cyan(),
    ]))
}

fn token_usage_segment(props: &FooterProps) -> Option<Line<'static>> {
    let input = props.input_tokens.unwrap_or(0);
    let output = props.output_tokens.unwrap_or(0);
    let cached = props.cached_tokens.unwrap_or(0);
    let total = input.saturating_add(output).saturating_add(cached);
    if total == 0 {
        return None;
    }

    let total_fmt = format_si_suffix(total);
    let mut spans = vec![
        "Tokens: ".dim(),
        Span::from(format!("{total_fmt} total")).dim(),
    ];
    if cached > 0 {
        let cached_fmt = format_si_suffix(cached);
        spans.push(Span::from(format!(" ({cached_fmt} cached)")).dim());
    }

    Some(Line::from(spans))
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
    Skills,
    ShellCommand,
    PasteImage,
    EditPrevious,
    OpenEditor,
    Quit,
    ShowTranscript,
    TogglePlanDrawer,
    ToggleScreen,
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
        id: ShortcutId::Skills,
        bindings: &[ShortcutBinding {
            key: key_hint::plain(KeyCode::Char('$')),
            condition: DisplayCondition::Always,
        }],
        prefix: "",
        label: " for skills",
    },
    ShortcutDescriptor {
        id: ShortcutId::ShellCommand,
        bindings: &[ShortcutBinding {
            key: key_hint::plain(KeyCode::Char('!')),
            condition: DisplayCondition::Always,
        }],
        prefix: "",
        label: " for shell commands",
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
            key: key_hint::alt(KeyCode::Char('p')),
            condition: DisplayCondition::Always,
        }],
        prefix: "",
        label: " to toggle plan drawer",
    },
    ShortcutDescriptor {
        id: ShortcutId::ToggleScreen,
        bindings: &[ShortcutBinding {
            key: key_hint::ctrl(KeyCode::Char('l')),
            condition: DisplayCondition::Always,
        }],
        prefix: "",
        label: " to toggle full screen",
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;
    use pretty_assertions::assert_eq;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Modifier;

    /// Test baseline for established status segments. Atomic context variants
    /// stay off here because their dedicated integration test covers them
    /// without duplicating values across unrelated snapshots.
    fn snapshot_segment_config() -> FooterSegmentConfig {
        FooterSegmentConfig {
            prompt_summary: true,
            session_title: true,
            vim_mode: true,
            git_branch: true,
            worktree_name: true,
            git_stats: true,
            context: true,
            context_used_percent: false,
            context_remaining_percent: false,
            context_used_tokens: false,
            context_remaining_tokens: false,
            context_window_tokens: false,
            approval_mode: true,
            skillset: true,
            nori_version: true,
            token_usage: true,
            mode_indicator: true,
            cloud_session: true,
            composer_hints: true,
        }
    }

    fn default_props() -> FooterProps {
        FooterProps {
            mode: FooterMode::ShortcutSummary,
            esc_backtrack_hint: false,
            use_shift_enter_hint: false,
            is_task_running: false,
            vertical_footer: false,
            context_window_percent: None,
            context_tokens: None,
            context_window_tokens: None,
            git_branch: None,
            approval_mode_label: None,
            active_skillsets: Vec::new(),
            nori_version: None,
            nori_version_source: None,
            git_lines_added: None,
            git_lines_removed: None,
            git_has_untracked: false,
            is_worktree: false,
            input_tokens: None,
            output_tokens: None,
            cached_tokens: None,
            vim_mode_state: None,
            prompt_summary: None,
            session_title: None,
            worktree_name: None,
            footer_segment_config: snapshot_segment_config(),
            footer_layout_config: nori_config::FooterLayoutConfig::default(),
            acp_mode_label: None,
            cloud_session: None,
            host_status: None,
            composer_hints: None,
        }
    }

    fn snapshot_footer(name: &str, props: FooterProps) {
        snapshot_footer_at_width(name, 80, props);
    }

    fn snapshot_footer_at_width(name: &str, width: u16, props: FooterProps) {
        let height = footer_height(&props).max(1);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
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

    fn render_footer_row_text(props: &FooterProps, width: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
        terminal
            .draw(|f| {
                let area = Rect::new(0, 0, width, 1);
                render_footer(area, f.buffer_mut(), props);
            })
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        (0..width).map(|x| buffer[(x, 0)].symbol()).collect()
    }

    fn layout_from_toml(source: &str) -> FooterLayoutConfig {
        let config: nori_config::NoriConfigToml =
            toml::from_str(source).expect("footer layout should deserialize");
        FooterLayoutConfig::from_toml(&config.tui.footer_layout)
    }

    /// Props whose layout keeps the mode indicator on the footer row. The
    /// shipped default hangs it off the textarea's top-right corner, which
    /// `render_footer` does not draw, so the tests that exercise the
    /// indicator's own rendering opt it back into the footer.
    fn props_with_mode_indicator_in_footer() -> FooterProps {
        FooterProps {
            footer_layout_config: layout_from_toml(
                r#"
[tui.footer_layout]
footer_left = ["git_branch"]
footer_right = ["mode_indicator"]
"#,
            ),
            ..default_props()
        }
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
    fn footer_with_untracked_alert() {
        snapshot_footer(
            "footer_with_untracked_alert",
            FooterProps {
                git_branch: Some("main".to_string()),
                git_has_untracked: true,
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
    fn the_default_footer_shows_a_mode_that_is_not_agent() {
        let shown = |label: &str| {
            render_footer_text(FooterProps {
                git_branch: Some("main".to_string()),
                approval_mode_label: Some(label.to_string()),
                footer_segment_config: FooterSegmentConfig::default(),
                footer_layout_config: FooterLayoutConfig::default(),
                ..default_props()
            })
        };

        assert!(shown("Ask").contains("Approvals: Ask"));
        assert!(shown("Approve all").contains("Approvals: Approve all"));
        assert!(!shown("Agent").contains("Approvals"));
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
    fn footer_with_mode_indicator_uses_right_aligned_footer_segment() {
        let rendered = render_footer_text(FooterProps {
            git_branch: Some("main".to_string()),
            acp_mode_label: Some("Plan".to_string()),
            ..props_with_mode_indicator_in_footer()
        });

        let first_line = rendered.lines().next().unwrap_or_default();
        assert!(first_line.ends_with("[ Plan ]"));
        assert!(first_line.contains("⎇ main"));
        assert!(!first_line.contains("Mode:"));
    }

    #[test]
    fn default_idle_footer_shows_only_location_and_headroom() {
        let props = FooterProps {
            git_branch: Some("main".to_string()),
            context_window_percent: Some(44),
            context_tokens: Some(116_000),
            context_window_tokens: Some(272_000),
            // Values the defaults deliberately drop: available, but not shown.
            approval_mode_label: Some("Agent".to_string()),
            active_skillsets: vec!["clifford".to_string()],
            nori_version: Some("19.1.1".to_string()),
            nori_version_source: Some(NoriVersionSource::Skillsets),
            session_title: Some("Fix login flakes".to_string()),
            input_tokens: Some(64_000),
            output_tokens: Some(13_000),
            cached_tokens: Some(32_000),
            acp_mode_label: Some("Plan".to_string()),
            footer_segment_config: FooterSegmentConfig::default(),
            footer_layout_config: FooterLayoutConfig::default(),
            ..default_props()
        };
        let rendered = render_footer_text(props.clone());

        // Branch and context, right-aligned, and nothing else.
        assert_eq!(rendered.trim(), "⎇ main · 44% / 272k");
        assert!(
            rendered.ends_with("⎇ main · 44% / 272k"),
            "footer metadata should be right-aligned: {rendered:?}"
        );
        for clutter in ["Approvals", "Skillset", "Skillsets v", "Title:", "Tokens:"] {
            assert!(
                !rendered.contains(clutter),
                "default footer should not contain {clutter:?}: {rendered:?}"
            );
        }

        // The mode indicator moved above the prompt rather than disappearing.
        assert_eq!(
            textarea_corner_segments(&props).top_right.to_string(),
            "[ Plan ]"
        );
    }

    #[test]
    fn default_footer_left_stays_empty_until_self_hiding_state_is_true() {
        let base = FooterProps {
            git_branch: Some("main".to_string()),
            footer_segment_config: FooterSegmentConfig::default(),
            footer_layout_config: FooterLayoutConfig::default(),
            ..default_props()
        };
        let groups = footer_segment_groups(&base);
        assert!(
            groups.left.is_empty(),
            "an ordinary local shell should leave the left group empty: {:?}",
            groups.left
        );

        let attached = FooterProps {
            cloud_session: Some("nori-fast-kazunoko-aac8".to_string()),
            vim_mode_state: Some(VimModeState::Insert),
            ..base
        };
        let rendered = render_footer_text(attached);
        assert!(rendered.contains("nori-fast-kazunoko-aac8"), "{rendered:?}");
        assert!(rendered.contains("INSERT"), "{rendered:?}");
    }

    #[test]
    fn enabling_a_default_off_segment_needs_no_layout_override() {
        // The default layout keeps a home for every default-off segment, so a
        // single `[tui.footer_segments]` line is enough to bring one back.
        let rendered = render_footer_text(FooterProps {
            approval_mode_label: Some("Agent".to_string()),
            footer_segment_config: FooterSegmentConfig {
                approval_mode: true,
                ..FooterSegmentConfig::default()
            },
            footer_layout_config: FooterLayoutConfig::default(),
            ..default_props()
        });

        assert_eq!(rendered.trim(), "Approvals: Agent");
    }

    #[test]
    fn narrow_footer_sheds_right_segments_before_overwriting_the_left_group() {
        let props = FooterProps {
            git_branch: Some("some-quite-long-branch-name".to_string()),
            context_window_percent: Some(44),
            context_tokens: Some(116_000),
            context_window_tokens: Some(272_000),
            cloud_session: Some("nori-fast-kazunoko-aac8".to_string()),
            footer_segment_config: FooterSegmentConfig::default(),
            footer_layout_config: FooterLayoutConfig::default(),
            ..default_props()
        };

        // Wide enough for everything.
        let wide = render_footer_row_text(&props, 80);
        assert!(wide.starts_with("  ☁ nori-fast-kazunoko-aac8"), "{wide:?}");
        assert!(
            wide.trim_end()
                .ends_with("⎇ some-quite-long-branch-name · 44% / 272k"),
            "{wide:?}"
        );

        // Narrower: the trailing context segment goes first.
        let squeezed = render_footer_row_text(&props, 60);
        assert!(
            squeezed.contains("☁ nori-fast-kazunoko-aac8"),
            "{squeezed:?}"
        );
        assert!(!squeezed.contains("44% / 272k"), "{squeezed:?}");

        // Narrower still: the right group disappears and the left group
        // survives intact rather than being painted over.
        let tiny = render_footer_row_text(&props, 32);
        assert_eq!(tiny.trim_end(), "  ☁ nori-fast-kazunoko-aac8");
    }

    #[test]
    fn narrow_footer_hides_the_right_group_rather_than_clipping_it() {
        let props = FooterProps {
            git_branch: Some("main".to_string()),
            footer_segment_config: FooterSegmentConfig::default(),
            footer_layout_config: FooterLayoutConfig::default(),
            ..default_props()
        };

        assert_eq!(render_footer_row_text(&props, 12).trim(), "⎇ main");
        // One column short of fitting: nothing rendered, no half-drawn branch.
        assert_eq!(render_footer_row_text(&props, 5).trim(), "");
    }

    #[test]
    fn footer_skips_mode_indicator_without_agent_mode() {
        let rendered = render_footer_text(FooterProps {
            git_branch: Some("main".to_string()),
            ..props_with_mode_indicator_in_footer()
        });

        assert!(!rendered.contains("[ Plan ]"));
        assert!(!rendered.contains("Mode"));
    }

    #[test]
    fn footer_mode_indicator_truncates_long_labels() {
        let rendered = render_footer_text(FooterProps {
            git_branch: Some("main".to_string()),
            acp_mode_label: Some("ABCDEFGHIJKLMNOPQRSTUV".to_string()),
            ..props_with_mode_indicator_in_footer()
        });

        let first_line = rendered.lines().next().unwrap_or_default();
        assert!(first_line.ends_with("[ ABCDEFGHIJKLMNOPQRS… ]"));
        assert!(!first_line.contains("ABCDEFGHIJKLMNOPQRSTUV"));
    }

    fn mode_indicator_fg(label: &str) -> ratatui::style::Color {
        let props = FooterProps {
            git_branch: Some("main".to_string()),
            acp_mode_label: Some(label.to_string()),
            ..props_with_mode_indicator_in_footer()
        };
        let height = footer_height(&props).max(1);
        let mut terminal = Terminal::new(TestBackend::new(80, height)).unwrap();
        terminal
            .draw(|f| {
                let area = Rect::new(0, 0, f.area().width, height);
                render_footer(area, f.buffer_mut(), &props);
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let bracket = format!("[ {label} ]");
        let row = &buffer.content[..buffer.area.width as usize];
        let text: String = row
            .iter()
            .map(|c| c.symbol().chars().next().unwrap_or(' '))
            .collect();
        let offset = text
            .find(&bracket)
            .expect("mode indicator bracket not found");
        row[offset].fg
    }

    #[test]
    fn footer_mode_indicator_plan_is_green() {
        assert_eq!(mode_indicator_fg("Plan"), ratatui::style::Color::Green);
    }

    #[test]
    fn footer_mode_indicator_plan_case_insensitive() {
        assert_eq!(mode_indicator_fg("PLAN MODE"), ratatui::style::Color::Green);
    }

    #[test]
    fn footer_mode_indicator_bypass_is_red() {
        assert_eq!(mode_indicator_fg("Bypass"), ratatui::style::Color::Red);
    }

    #[test]
    fn footer_mode_indicator_accept_is_cyan() {
        assert_eq!(mode_indicator_fg("Accept All"), ratatui::style::Color::Cyan);
    }

    #[test]
    fn footer_mode_indicator_implement_is_cyan() {
        assert_eq!(mode_indicator_fg("Implement"), ratatui::style::Color::Cyan);
    }

    #[test]
    fn footer_mode_indicator_build_is_cyan() {
        assert_eq!(mode_indicator_fg("Build"), ratatui::style::Color::Cyan);
    }

    #[test]
    fn footer_mode_indicator_unknown_is_default() {
        assert_eq!(
            mode_indicator_fg("Custom Mode"),
            ratatui::style::Color::Reset
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
                nori_version_source: Some(NoriVersionSource::LegacySkillsets),
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
                nori_version_source: Some(NoriVersionSource::LegacySkillsets),
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
    fn footer_context_falls_back_to_percent_when_window_size_is_unavailable() {
        let rendered = render_footer_text(FooterProps {
            context_window_percent: Some(16),
            context_tokens: Some(42_600),
            footer_segment_config: FooterSegmentConfig::default(),
            ..default_props()
        });

        assert_eq!(rendered.trim(), "16%");
    }

    #[test]
    fn footer_context_uses_percentage_and_window_size_by_default() {
        let rendered = render_footer_text(FooterProps {
            context_window_percent: Some(44),
            context_tokens: Some(116_000),
            context_window_tokens: Some(272_000),
            footer_segment_config: FooterSegmentConfig::default(),
            ..default_props()
        });

        assert_eq!(rendered.trim(), "44% / 272k");
    }

    #[test]
    fn custom_footer_segments_work_in_every_layout_placement() {
        for placement in [
            "footer_left",
            "footer_right",
            "textarea_top_left",
            "textarea_top_right",
            "textarea_bottom_left",
            "textarea_bottom_right",
        ] {
            let layout = layout_from_toml(&format!(
                r#"
[tui.footer_layout]
{placement} = [{{ format = "{{git_branch}}" }}]
"#
            ));
            let props = FooterProps {
                git_branch: Some("main".to_string()),
                footer_segment_config: FooterSegmentConfig {
                    git_branch: false,
                    ..snapshot_segment_config()
                },
                footer_layout_config: layout,
                ..default_props()
            };
            let rendered = match placement {
                "footer_left" | "footer_right" => render_footer_text(props),
                "textarea_top_left" => textarea_corner_segments(&props).top_left.to_string(),
                "textarea_top_right" => textarea_corner_segments(&props).top_right.to_string(),
                "textarea_bottom_left" => textarea_corner_segments(&props).bottom_left.to_string(),
                "textarea_bottom_right" => {
                    textarea_corner_segments(&props).bottom_right.to_string()
                }
                _ => unreachable!("all placements are covered"),
            };

            assert!(
                rendered.contains("⎇ main"),
                "custom segment should render in {placement}: {rendered:?}"
            );
        }
    }

    #[test]
    fn custom_footer_segment_composes_text_and_builtin_segments() {
        let layout = layout_from_toml(
            r#"
[tui.footer_layout]
footer_left = [
    { format = "{git_branch} — {approval_mode}" },
]
"#,
        );
        let rendered = render_footer_text(FooterProps {
            git_branch: Some("main".to_string()),
            approval_mode_label: Some("Agent".to_string()),
            footer_segment_config: FooterSegmentConfig {
                git_branch: false,
                approval_mode: false,
                ..snapshot_segment_config()
            },
            footer_layout_config: layout,
            ..default_props()
        });

        assert_eq!(rendered.trim(), "⎇ main — Approvals: Agent");
    }

    #[test]
    fn custom_footer_segment_preserves_builtin_styles_and_literal_braces() {
        let layout = layout_from_toml(
            r#"
[tui.footer_layout]
footer_left = [
    { format = "{git_branch} {{ready}} {approval_mode}" },
]
"#,
        );
        let props = FooterProps {
            git_branch: Some("main".to_string()),
            approval_mode_label: Some("Agent".to_string()),
            footer_layout_config: layout,
            ..default_props()
        };
        let segment = segments_for(&props, &props.footer_layout_config.footer_left)
            .into_iter()
            .next()
            .expect("custom segment should render");

        assert_eq!(segment.to_string(), "⎇ main {ready} Approvals: Agent");
        assert_eq!(
            segment.spans[0].style.fg,
            Some(ratatui::style::Color::Yellow)
        );
        assert_eq!(segment.spans[2].style.fg, None);
        assert_eq!(
            segment.spans[3].style.fg,
            Some(ratatui::style::Color::Magenta)
        );
    }

    #[test]
    fn custom_footer_segment_hides_when_a_referenced_segment_is_unavailable() {
        let layout = layout_from_toml(
            r#"
[tui.footer_layout]
footer_left = [
    { format = "{git_branch} — {approval_mode}" },
]
footer_right = []
"#,
        );
        let rendered = render_footer_text(FooterProps {
            git_branch: Some("main".to_string()),
            approval_mode_label: None,
            footer_layout_config: layout,
            ..default_props()
        });

        assert_eq!(rendered.trim(), "");
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
    fn footer_with_session_title() {
        snapshot_footer(
            "footer_with_session_title",
            FooterProps {
                session_title: Some("Fix login flakes".to_string()),
                git_branch: Some("main".to_string()),
                ..default_props()
            },
        );
    }

    #[test]
    fn footer_session_title_truncates_to_share_the_line() {
        // The value arriving here is already bounded to MAX_TITLE_DISPLAY_CHARS
        // (48); the footer caps it again so git, context, and approvals still
        // fit alongside it.
        let rendered = render_footer_text(FooterProps {
            session_title: Some("Continue working toward the active thread goal.".to_string()),
            git_branch: Some("main".to_string()),
            ..default_props()
        });
        assert!(
            rendered.contains("Title: Continue working toward…"),
            "long session titles should truncate in the footer, got:\n{rendered}"
        );
    }

    #[test]
    fn footer_with_worktree_name() {
        snapshot_footer_at_width(
            "footer_with_worktree_name",
            120,
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
            ..snapshot_segment_config()
        };

        snapshot_footer_at_width(
            "footer_with_worktree_name_disabled",
            120,
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
            ..snapshot_segment_config()
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
            ..snapshot_segment_config()
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
            session_title: false,
            vim_mode: false,
            git_branch: false,
            worktree_name: false,
            git_stats: false,
            context: false,
            context_used_percent: false,
            context_remaining_percent: false,
            context_used_tokens: false,
            context_remaining_tokens: false,
            context_window_tokens: false,
            approval_mode: false,
            skillset: false,
            nori_version: false,
            token_usage: false,
            mode_indicator: false,
            cloud_session: false,
            composer_hints: false,
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
    fn footer_cloud_session_renders_id() {
        // When attached to a cloud session, the footer shows the cloud session
        // identity so the user can tell which remote VM they are driving.
        let rendered = render_footer_text(FooterProps {
            cloud_session: Some("nori-fast-kazunoko-aac8".to_string()),
            ..default_props()
        });

        assert!(
            rendered.contains("nori-fast-kazunoko-aac8"),
            "footer with a cloud session must render the session id, got:\n{rendered}"
        );
    }

    #[test]
    fn footer_without_cloud_session_omits_badge() {
        // Guard: a local session (no cloud identity) must not render a cloud
        // badge. A UUID-style id from a local ACP agent must never appear.
        let rendered = render_footer_text(FooterProps {
            git_branch: Some("main".to_string()),
            cloud_session: None,
            ..default_props()
        });

        assert!(
            !rendered.contains('☁'),
            "footer without a cloud session must not render a cloud badge, got:\n{rendered}"
        );
    }

    #[test]
    fn footer_with_cloud_session() {
        snapshot_footer(
            "footer_with_cloud_session",
            FooterProps {
                git_branch: Some("main".to_string()),
                cloud_session: Some("nori-fast-kazunoko-aac8".to_string()),
                ..default_props()
            },
        );
    }

    #[test]
    fn footer_vertical_with_segments_disabled() {
        let segment_config = FooterSegmentConfig {
            skillset: false,
            nori_version: false,
            ..snapshot_segment_config()
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

    fn hint_props(hints: ComposerHints) -> FooterProps {
        FooterProps {
            composer_hints: Some(hints),
            footer_segment_config: FooterSegmentConfig::default(),
            ..default_props()
        }
    }

    fn idle_controller() -> ComposerHints {
        ComposerHints {
            draft: HintDraft::Empty,
            can_submit: true,
            own_turn: false,
            steering: false,
            queue_paused: false,
            shell: true,
            files: true,
        }
    }

    fn hint_text(props: &FooterProps, width: u16) -> String {
        render_footer_row_text(props, width).trim_end().to_string()
    }

    #[test]
    fn empty_prompt_shows_discovery_hints() {
        assert_eq!(
            hint_text(&hint_props(idle_controller()), 80),
            "  / commands · ! shell · @ files · ? help"
        );
    }

    #[test]
    fn typed_text_shows_send_and_newline_keys() {
        let typed = ComposerHints {
            draft: HintDraft::Message,
            ..idle_controller()
        };
        assert_eq!(
            hint_text(&hint_props(typed), 80),
            "  Enter send · Ctrl+J newline"
        );
        let enhanced = FooterProps {
            use_shift_enter_hint: true,
            ..hint_props(typed)
        };
        assert_eq!(
            hint_text(&enhanced, 80),
            "  Enter send · Shift+Enter newline"
        );
    }

    #[test]
    fn own_turn_advertises_steering_and_queue_only_when_valid() {
        let steering = ComposerHints {
            draft: HintDraft::Message,
            own_turn: true,
            steering: true,
            ..idle_controller()
        };
        assert_eq!(
            hint_text(&hint_props(steering), 80),
            "  Enter steer · Tab queue · Ctrl+J newline"
        );
        let command = ComposerHints {
            draft: HintDraft::Command,
            ..steering
        };
        assert_eq!(hint_text(&hint_props(command), 80), "  Enter run");
        let no_steering = ComposerHints {
            steering: false,
            ..steering
        };
        assert_eq!(
            hint_text(&hint_props(no_steering), 80),
            "  Enter queue · Ctrl+J newline"
        );
    }

    #[test]
    fn paused_queue_leads_with_resume() {
        let paused = ComposerHints {
            queue_paused: true,
            ..idle_controller()
        };
        assert_eq!(
            hint_text(&hint_props(paused), 80),
            "  Enter resume queue · / commands · ! shell · @ files · ? help"
        );
    }

    #[test]
    fn watcher_hints_never_offer_sending() {
        let watcher = ComposerHints {
            can_submit: false,
            queue_paused: true,
            ..idle_controller()
        };
        assert_eq!(
            hint_text(&hint_props(watcher), 80),
            "  / commands · @ files · ? help"
        );
        let watcher_typing = ComposerHints {
            draft: HintDraft::Message,
            own_turn: true,
            ..watcher
        };
        assert_eq!(
            hint_text(&hint_props(watcher_typing), 80),
            "  Ctrl+J newline"
        );
        let watcher_command = ComposerHints {
            draft: HintDraft::Command,
            ..watcher
        };
        assert_eq!(hint_text(&hint_props(watcher_command), 80), "  Enter run");
    }

    #[test]
    fn unavailable_shell_and_files_are_not_advertised() {
        let remote = ComposerHints {
            shell: false,
            files: false,
            ..idle_controller()
        };
        assert_eq!(hint_text(&hint_props(remote), 80), "  / commands · ? help");
        let shell_draft = ComposerHints {
            draft: HintDraft::Shell { empty: false },
            ..idle_controller()
        };
        assert_eq!(hint_text(&hint_props(shell_draft), 80), "  Enter run");
        let no_shell = ComposerHints {
            shell: false,
            ..shell_draft
        };
        assert_eq!(hint_text(&hint_props(no_shell), 80), "");
        let shell_mode_empty = ComposerHints {
            draft: HintDraft::Shell { empty: true },
            ..idle_controller()
        };
        assert_eq!(hint_text(&hint_props(shell_mode_empty), 80), "");
    }

    #[test]
    fn hint_keys_are_bold_and_labels_dim() {
        let props = hint_props(idle_controller());
        let mut terminal = Terminal::new(TestBackend::new(60, 1)).unwrap();
        terminal
            .draw(|f| render_footer(Rect::new(0, 0, 60, 1), f.buffer_mut(), &props))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let cell_of = |symbol: &str| {
            (0..60)
                .map(|x| &buffer[(x, 0)])
                .find(|cell| cell.symbol() == symbol)
                .unwrap_or_else(|| panic!("{symbol} is drawn"))
        };
        for key in ["/", "?"] {
            assert!(cell_of(key).modifier.contains(Modifier::BOLD), "{key}");
        }
        let label = cell_of("h");
        assert!(label.modifier.contains(Modifier::DIM));
        assert!(!label.modifier.contains(Modifier::BOLD));
    }

    fn cols(text: &str) -> u16 {
        u16::try_from(unicode_width::UnicodeWidthStr::width(text)).unwrap()
    }

    #[test]
    fn narrow_terminal_drops_whole_hints_from_the_end() {
        let props = FooterProps {
            git_branch: Some("main".to_string()),
            ..hint_props(idle_controller())
        };
        assert_eq!(
            hint_text(&props, 80).split_whitespace().last(),
            Some("main")
        );
        // Footer metadata outlasts the hints.
        let with_branch = "  / commands · ! shell  ⎇ main";
        assert_eq!(hint_text(&props, cols(with_branch)), with_branch);
        let narrower = cols(with_branch) - 1;
        let gap = usize::from(narrower - cols("  / commands") - cols("⎇ main"));
        assert_eq!(
            hint_text(&props, narrower),
            format!("  / commands{}⎇ main", " ".repeat(gap))
        );
        let props = hint_props(idle_controller());
        let three = "  / commands · ! shell · @ files";
        assert_eq!(hint_text(&props, cols(three)), three);
        assert_eq!(hint_text(&props, cols(three) - 1), "  / commands · ! shell");
        let one = "  / commands";
        assert_eq!(hint_text(&props, cols(one)), one);
        assert_eq!(hint_text(&props, cols(one) - 1), "");
    }

    #[test]
    fn a_draft_outranks_a_paused_queue() {
        let props = hint_props(ComposerHints {
            draft: HintDraft::Message,
            queue_paused: true,
            ..idle_controller()
        });
        assert_eq!(hint_text(&props, 80), "  Enter send · Ctrl+J newline");
    }

    #[test]
    fn hints_lead_the_host_status() {
        let props = FooterProps {
            host_status: Some(Line::from("watching · read-only")),
            ..hint_props(ComposerHints {
                can_submit: false,
                ..idle_controller()
            })
        };
        assert_eq!(
            hint_text(&props, 80),
            "  / commands · @ files · ? help · watching · read-only"
        );
        let one = "  / commands · watching · read-only";
        assert_eq!(hint_text(&props, cols(one)), one);
        assert_eq!(hint_text(&props, cols(one) - 1), "  watching · read-only");
    }

    #[test]
    fn turning_hints_off_or_an_explicit_layout_hides_them() {
        let mut off = hint_props(idle_controller());
        off.footer_segment_config
            .set_enabled(FooterSegment::ComposerHints, false);
        assert_eq!(hint_text(&off, 80), "");
        let explicit = FooterProps {
            footer_layout_config: layout_from_toml(
                r#"
[tui.footer_layout]
footer_left = ["git_branch"]
"#,
            ),
            git_branch: Some("main".to_string()),
            ..hint_props(idle_controller())
        };
        assert_eq!(hint_text(&explicit, 80), "  ⎇ main");
    }

    #[test]
    fn temporary_messages_replace_hints() {
        let props = FooterProps {
            mode: FooterMode::CtrlCReminder,
            ..hint_props(idle_controller())
        };
        assert_eq!(hint_text(&props, 80), "  ctrl + c again to quit");
    }
}

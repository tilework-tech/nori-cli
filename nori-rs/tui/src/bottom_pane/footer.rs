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
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Widget;

const MODE_INDICATOR_LABEL_MAX_CHARS: usize = 20;

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
    left: Line<'static>,
    right: Line<'static>,
}

impl FooterRow {
    fn left(left: Line<'static>) -> Self {
        Self {
            left,
            right: Line::from(""),
        }
    }

    fn joined(self) -> Line<'static> {
        if self.right.spans.is_empty() {
            self.left
        } else if self.left.spans.is_empty() {
            self.right
        } else {
            let mut line = self.left;
            line.push_span(" · ".dim());
            line.extend(self.right.spans);
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
    row.left.render(left_area, buf);

    let right_width = row.right.width() as u16;
    if right_width == 0 {
        return;
    }

    let right_area = Rect {
        x: area.right().saturating_sub(right_width),
        y: area.y,
        width: right_width.min(area.width),
        height: 1,
    };
    row.right.render(right_area, buf);
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
                footer_segments
                    .left
                    .into_iter()
                    .chain(footer_segments.right)
                    .map(FooterRow::left)
                    .collect()
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
                let segments = footer_segments
                    .left
                    .into_iter()
                    .chain(footer_segments.right)
                    .collect::<Vec<_>>();
                if segments.is_empty() {
                    Vec::new()
                } else {
                    segments.into_iter().map(FooterRow::left).collect()
                }
            } else {
                footer_row_for_segment_groups(footer_segments)
            }
        }
    }
}

fn footer_row_for_segment_groups(footer_segments: FooterSegmentGroups) -> Vec<FooterRow> {
    let row = FooterRow {
        left: join_footer_segments(&footer_segments.left),
        right: join_footer_segments(&footer_segments.right),
    };
    if row.left.spans.is_empty() && row.right.spans.is_empty() {
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
    right: Vec<Line<'static>>,
}

fn footer_segment_groups(props: &FooterProps) -> FooterSegmentGroups {
    FooterSegmentGroups {
        left: segments_for(props, &props.footer_layout_config.footer_left),
        right: segments_for(props, &props.footer_layout_config.footer_right),
    }
}

fn segments_for(props: &FooterProps, segment_order: &[FooterLayoutItem]) -> Vec<Line<'static>> {
    segment_order
        .iter()
        .filter_map(|item| footer_layout_item(props, item))
        .collect()
}

fn footer_layout_item(props: &FooterProps, item: &FooterLayoutItem) -> Option<Line<'static>> {
    match item {
        FooterLayoutItem::Builtin(segment) if props.footer_segment_config.is_enabled(*segment) => {
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
            let label = mode_indicator_label(raw_label);
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
    }
}

fn mode_indicator_label(label: &str) -> String {
    if label.chars().count() <= MODE_INDICATOR_LABEL_MAX_CHARS {
        return label.to_string();
    }

    label
        .chars()
        .take(MODE_INDICATOR_LABEL_MAX_CHARS.saturating_sub(1))
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
    let formatted_used_tokens = props
        .context_tokens
        .filter(|&tokens| tokens > 0)
        .map(format_context_tokens);
    let formatted_window_tokens = props
        .context_window_tokens
        .filter(|&tokens| tokens > 0)
        .map(format_context_tokens);
    let context_text = match (
        props.context_window_percent.map(clamp_percent),
        formatted_window_tokens,
        formatted_used_tokens,
    ) {
        (Some(percent), Some(window), _) => Some(format!("{percent}% / {window}")),
        (Some(percent), None, _) => Some(format!("{percent}%")),
        (None, _, Some(used)) => Some(used),
        (None, _, None) => None,
    };
    context_text.map(Line::from)
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
    let mut formatted = format_si_suffix(tokens);
    if formatted.ends_with('K') {
        formatted.pop();
        formatted.push('k');
    }
    formatted
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

    /// Test baseline for established status segments. Atomic context variants
    /// stay off here because their dedicated integration test covers them
    /// without duplicating values across unrelated snapshots.
    fn snapshot_segment_config() -> FooterSegmentConfig {
        FooterSegmentConfig {
            prompt_summary: true,
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
            worktree_name: None,
            footer_segment_config: snapshot_segment_config(),
            footer_layout_config: nori_config::FooterLayoutConfig::default(),
            acp_mode_label: None,
            cloud_session: None,
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

    fn layout_from_toml(source: &str) -> FooterLayoutConfig {
        let config: nori_config::NoriConfigToml =
            toml::from_str(source).expect("footer layout should deserialize");
        FooterLayoutConfig::from_toml(&config.tui.footer_layout)
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
            ..default_props()
        });

        let first_line = rendered.lines().next().unwrap_or_default();
        assert!(first_line.ends_with("[ Plan ]"));
        assert!(first_line.contains("⎇ main"));
        assert!(!first_line.contains("Mode:"));
    }

    #[test]
    fn footer_skips_mode_indicator_without_agent_mode() {
        let rendered = render_footer_text(FooterProps {
            git_branch: Some("main".to_string()),
            ..default_props()
        });

        assert!(!rendered.contains("[ Plan ]"));
        assert!(!rendered.contains("Mode"));
    }

    #[test]
    fn footer_mode_indicator_truncates_long_labels() {
        let rendered = render_footer_text(FooterProps {
            git_branch: Some("main".to_string()),
            acp_mode_label: Some("ABCDEFGHIJKLMNOPQRSTUV".to_string()),
            ..default_props()
        });

        let first_line = rendered.lines().next().unwrap_or_default();
        assert!(first_line.ends_with("[ ABCDEFGHIJKLMNOPQRS… ]"));
        assert!(!first_line.contains("ABCDEFGHIJKLMNOPQRSTUV"));
    }

    fn mode_indicator_fg(label: &str) -> ratatui::style::Color {
        let props = FooterProps {
            git_branch: Some("main".to_string()),
            acp_mode_label: Some(label.to_string()),
            ..default_props()
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
            ..snapshot_segment_config()
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
}

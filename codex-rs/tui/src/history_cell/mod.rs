use crate::diff_render::create_diff_summary;
use crate::diff_render::display_path_for;
use crate::exec_cell::CommandOutput;
use crate::exec_cell::OutputLinesParams;
use crate::exec_cell::TOOL_CALL_MAX_LINES;
use crate::exec_cell::output_lines;
use crate::exec_cell::spinner;
use crate::exec_command::relativize_to_home;
use crate::exec_command::strip_bash_lc_and_escape;
use crate::markdown::append_markdown;
use crate::render::line_utils::line_to_static;
use crate::render::line_utils::prefix_lines;
use crate::render::line_utils::push_owned_lines;
use crate::render::renderable::Renderable;
use crate::style::user_message_style;
use crate::text_formatting::format_and_truncate_tool_result;
use crate::text_formatting::truncate_text;
use crate::ui_consts::LIVE_PREFIX_COLS;
use crate::update_action::UpdateAction;
use crate::version::CODEX_CLI_VERSION;
use crate::wrapping::RtOptions;
use crate::wrapping::word_wrap_line;
use crate::wrapping::word_wrap_lines;
use base64::Engine;
use codex_common::format_env_display::format_env_display;
use codex_core::config::Config;
use codex_core::config::types::McpServerTransportConfig;
use codex_core::config::types::ReasoningSummaryFormat;
use codex_core::protocol::FileChange;
use codex_core::protocol::McpAuthStatus;
use codex_core::protocol::McpInvocation;
use codex_core::protocol::SessionConfiguredEvent;
use codex_core::protocol_config_types::ReasoningEffort as ReasoningEffortConfig;
use codex_protocol::plan_tool::PlanItemArg;
use codex_protocol::plan_tool::StepStatus;
use codex_protocol::plan_tool::UpdatePlanArgs;
use image::DynamicImage;
use image::ImageReader;
use mcp_types::EmbeddedResourceResource;
use mcp_types::Resource;
use mcp_types::ResourceLink;
use mcp_types::ResourceTemplate;
use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::style::Styled;
use ratatui::style::Stylize;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;
use std::any::Any;
use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;
use tracing::error;
use unicode_width::UnicodeWidthStr;

/// Represents an event to display in the conversation history. Returns its
/// `Vec<Line<'static>>` representation to make it easier to display in a
/// scrollable list.
pub(crate) trait HistoryCell: std::fmt::Debug + Send + Sync + Any {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>>;

    fn desired_height(&self, width: u16) -> u16 {
        Paragraph::new(Text::from(self.display_lines(width)))
            .wrap(Wrap { trim: false })
            .line_count(width)
            .try_into()
            .unwrap_or(0)
    }

    fn transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
        self.display_lines(width)
    }

    fn desired_transcript_height(&self, width: u16) -> u16 {
        let lines = self.transcript_lines(width);
        // Workaround for ratatui bug: if there's only one line and it's whitespace-only, ratatui gives 2 lines.
        if let [line] = &lines[..]
            && line
                .spans
                .iter()
                .all(|s| s.content.chars().all(char::is_whitespace))
        {
            return 1;
        }

        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .line_count(width)
            .try_into()
            .unwrap_or(0)
    }

    fn is_stream_continuation(&self) -> bool {
        false
    }
}

impl Renderable for Box<dyn HistoryCell> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let lines = self.display_lines(area.width);
        let y = if area.height == 0 {
            0
        } else {
            let overflow = lines.len().saturating_sub(usize::from(area.height));
            u16::try_from(overflow).unwrap_or(u16::MAX)
        };
        Paragraph::new(Text::from(lines))
            .scroll((y, 0))
            .render(area, buf);
    }
    fn desired_height(&self, width: u16) -> u16 {
        HistoryCell::desired_height(self.as_ref(), width)
    }
}

impl dyn HistoryCell {
    pub(crate) fn as_any(&self) -> &dyn Any {
        self
    }

    pub(crate) fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[derive(Debug)]
pub(crate) struct UserHistoryCell {
    pub message: String,
}

impl HistoryCell for UserHistoryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'static>> = Vec::new();

        let wrap_width = width
            .saturating_sub(
                LIVE_PREFIX_COLS + 1, /* keep a one-column right margin for wrapping */
            )
            .max(1);

        let style = user_message_style();

        let wrapped = word_wrap_lines(
            self.message.lines().map(|l| Line::from(l).style(style)),
            // Wrap algorithm matches textarea.rs.
            RtOptions::new(usize::from(wrap_width))
                .wrap_algorithm(textwrap::WrapAlgorithm::FirstFit),
        );

        lines.push(Line::from("").style(style));
        lines.extend(prefix_lines(wrapped, "› ".bold().dim(), "  ".into()));
        lines.push(Line::from("").style(style));
        lines
    }
}

#[derive(Debug)]
pub(crate) struct ReasoningSummaryCell {
    _header: String,
    content: String,
    transcript_only: bool,
}

impl ReasoningSummaryCell {
    pub(crate) fn new(header: String, content: String, transcript_only: bool) -> Self {
        Self {
            _header: header,
            content,
            transcript_only,
        }
    }

    fn lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'static>> = Vec::new();
        append_markdown(
            &self.content,
            Some((width as usize).saturating_sub(2)),
            &mut lines,
        );
        let summary_style = Style::default().dim().italic();
        let summary_lines = lines
            .into_iter()
            .map(|mut line| {
                line.spans = line
                    .spans
                    .into_iter()
                    .map(|span| span.patch_style(summary_style))
                    .collect();
                line
            })
            .collect::<Vec<_>>();

        word_wrap_lines(
            &summary_lines,
            RtOptions::new(width as usize)
                .initial_indent("• ".dim().into())
                .subsequent_indent("  ".into()),
        )
    }
}

impl HistoryCell for ReasoningSummaryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        if self.transcript_only {
            Vec::new()
        } else {
            self.lines(width)
        }
    }

    fn desired_height(&self, width: u16) -> u16 {
        if self.transcript_only {
            0
        } else {
            self.lines(width).len() as u16
        }
    }

    fn transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
        self.lines(width)
    }

    fn desired_transcript_height(&self, width: u16) -> u16 {
        self.lines(width).len() as u16
    }
}

#[derive(Debug)]
pub(crate) struct AgentMessageCell {
    lines: Vec<Line<'static>>,
    is_first_line: bool,
}

impl AgentMessageCell {
    pub(crate) fn new(lines: Vec<Line<'static>>, is_first_line: bool) -> Self {
        Self {
            lines,
            is_first_line,
        }
    }
}

impl HistoryCell for AgentMessageCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        word_wrap_lines(
            &self.lines,
            RtOptions::new(width as usize)
                .initial_indent(if self.is_first_line {
                    "• ".dim().into()
                } else {
                    "  ".into()
                })
                .subsequent_indent("  ".into()),
        )
    }

    fn is_stream_continuation(&self) -> bool {
        !self.is_first_line
    }
}

#[derive(Debug)]
pub(crate) struct PlainHistoryCell {
    lines: Vec<Line<'static>>,
}

impl PlainHistoryCell {
    pub(crate) fn new(lines: Vec<Line<'static>>) -> Self {
        Self { lines }
    }
}

impl HistoryCell for PlainHistoryCell {
    fn display_lines(&self, _width: u16) -> Vec<Line<'static>> {
        self.lines.clone()
    }
}

#[cfg_attr(debug_assertions, allow(dead_code))]
#[derive(Debug)]
pub(crate) struct UpdateAvailableHistoryCell {
    latest_version: String,
    update_action: Option<UpdateAction>,
}

#[cfg_attr(debug_assertions, allow(dead_code))]
impl UpdateAvailableHistoryCell {
    pub(crate) fn new(latest_version: String, update_action: Option<UpdateAction>) -> Self {
        Self {
            latest_version,
            update_action,
        }
    }
}

impl HistoryCell for UpdateAvailableHistoryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        use ratatui_macros::line;
        use ratatui_macros::text;
        let update_instruction = if let Some(update_action) = self.update_action {
            line!["Run ", update_action.command_str().cyan(), " to update."]
        } else {
            line![
                "See ",
                "https://github.com/tilework-tech/nori-cli"
                    .cyan()
                    .underlined(),
                " for installation options."
            ]
        };

        let content = text![
            line![
                padded_emoji("✨").bold().cyan(),
                "Update available!".bold().cyan(),
                " ",
                format!("{CODEX_CLI_VERSION} -> {}", self.latest_version).bold(),
            ],
            update_instruction,
            "",
            "See full release notes:",
            "https://github.com/tilework-tech/nori-cli/releases/latest"
                .cyan()
                .underlined(),
        ];

        let inner_width = content
            .width()
            .min(usize::from(width.saturating_sub(4)))
            .max(1);
        with_border_with_inner_width(content.lines, inner_width)
    }
}

#[derive(Debug)]
pub(crate) struct PrefixedWrappedHistoryCell {
    text: Text<'static>,
    initial_prefix: Line<'static>,
    subsequent_prefix: Line<'static>,
}

impl PrefixedWrappedHistoryCell {
    pub(crate) fn new(
        text: impl Into<Text<'static>>,
        initial_prefix: impl Into<Line<'static>>,
        subsequent_prefix: impl Into<Line<'static>>,
    ) -> Self {
        Self {
            text: text.into(),
            initial_prefix: initial_prefix.into(),
            subsequent_prefix: subsequent_prefix.into(),
        }
    }
}

impl HistoryCell for PrefixedWrappedHistoryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        if width == 0 {
            return Vec::new();
        }
        let opts = RtOptions::new(width.max(1) as usize)
            .initial_indent(self.initial_prefix.clone())
            .subsequent_indent(self.subsequent_prefix.clone());
        let wrapped = word_wrap_lines(&self.text, opts);
        let mut out = Vec::new();
        push_owned_lines(&wrapped, &mut out);
        out
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.display_lines(width).len() as u16
    }
}

fn truncate_exec_snippet(full_cmd: &str) -> String {
    let mut snippet = match full_cmd.split_once('\n') {
        Some((first, _)) => format!("{first} ..."),
        None => full_cmd.to_string(),
    };
    snippet = truncate_text(&snippet, 80);
    snippet
}

fn exec_snippet(command: &[String]) -> String {
    let full_cmd = strip_bash_lc_and_escape(command);
    truncate_exec_snippet(&full_cmd)
}

pub fn new_approval_decision_cell(
    command: Vec<String>,
    decision: codex_core::protocol::ReviewDecision,
) -> Box<dyn HistoryCell> {
    use codex_core::protocol::ReviewDecision::*;

    let (symbol, summary): (Span<'static>, Vec<Span<'static>>) = match decision {
        Approved => {
            let snippet = Span::from(exec_snippet(&command)).dim();
            (
                "✔ ".green(),
                vec![
                    "You ".into(),
                    "approved".bold(),
                    " Nori to run".into(),
                    snippet,
                    " this time".bold(),
                ],
            )
        }
        ApprovedForSession => {
            let snippet = Span::from(exec_snippet(&command)).dim();
            (
                "✔ ".green(),
                vec![
                    "You ".into(),
                    "approved".bold(),
                    " Nori to run".into(),
                    snippet,
                    " every time this session".bold(),
                ],
            )
        }
        Denied => {
            let snippet = Span::from(exec_snippet(&command)).dim();
            (
                "✗ ".red(),
                vec![
                    "You ".into(),
                    "did not approve".bold(),
                    " Nori to run".into(),
                    snippet,
                ],
            )
        }
        Abort => {
            let snippet = Span::from(exec_snippet(&command)).dim();
            (
                "✗ ".red(),
                vec![
                    "You ".into(),
                    "canceled".bold(),
                    " the request to run ".into(),
                    snippet,
                ],
            )
        }
    };

    Box::new(PrefixedWrappedHistoryCell::new(
        Line::from(summary),
        symbol,
        "  ",
    ))
}

#[derive(Debug)]
pub(crate) struct PatchHistoryCell {
    changes: HashMap<PathBuf, FileChange>,
    cwd: PathBuf,
}

impl HistoryCell for PatchHistoryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        create_diff_summary(&self.changes, &self.cwd, width as usize)
    }
}

#[derive(Debug)]
struct CompletedMcpToolCallWithImageOutput {
    _image: DynamicImage,
}
impl HistoryCell for CompletedMcpToolCallWithImageOutput {
    fn display_lines(&self, _width: u16) -> Vec<Line<'static>> {
        vec!["tool result (image output)".into()]
    }
}

pub(crate) const SESSION_HEADER_MAX_INNER_WIDTH: usize = 56; // Just an eyeballed value

pub(crate) fn card_inner_width(width: u16, max_inner_width: usize) -> Option<usize> {
    if width < 4 {
        return None;
    }
    let inner_width = std::cmp::min(width.saturating_sub(4) as usize, max_inner_width);
    Some(inner_width)
}

/// Render `lines` inside a border sized to the widest span in the content.
pub(crate) fn with_border(lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    with_border_internal(lines, None)
}

/// Render `lines` inside a border whose inner width is at least `inner_width`.
///
/// This is useful when callers have already clamped their content to a
/// specific width and want the border math centralized here instead of
/// duplicating padding logic in the TUI widgets themselves.
pub(crate) fn with_border_with_inner_width(
    lines: Vec<Line<'static>>,
    inner_width: usize,
) -> Vec<Line<'static>> {
    with_border_internal(lines, Some(inner_width))
}

fn with_border_internal(
    lines: Vec<Line<'static>>,
    forced_inner_width: Option<usize>,
) -> Vec<Line<'static>> {
    let max_line_width = lines
        .iter()
        .map(|line| {
            line.iter()
                .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
                .sum::<usize>()
        })
        .max()
        .unwrap_or(0);
    let content_width = forced_inner_width
        .unwrap_or(max_line_width)
        .max(max_line_width);

    let mut out = Vec::with_capacity(lines.len() + 2);
    let border_inner_width = content_width + 2;
    out.push(vec![format!("╭{}╮", "─".repeat(border_inner_width)).dim()].into());

    for line in lines.into_iter() {
        let used_width: usize = line
            .iter()
            .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
            .sum();
        let span_count = line.spans.len();
        let mut spans: Vec<Span<'static>> = Vec::with_capacity(span_count + 4);
        spans.push(Span::from("│ ").dim());
        spans.extend(line.into_iter());
        if used_width < content_width {
            spans.push(Span::from(" ".repeat(content_width - used_width)).dim());
        }
        spans.push(Span::from(" │").dim());
        out.push(Line::from(spans));
    }

    out.push(vec![format!("╰{}╯", "─".repeat(border_inner_width)).dim()].into());

    out
}

/// Return the emoji followed by a hair space (U+200A).
/// Using only the hair space avoids excessive padding after the emoji while
/// still providing a small visual gap across terminals.
pub(crate) fn padded_emoji(emoji: &str) -> String {
    format!("{emoji}\u{200A}")
}

#[derive(Debug)]
pub struct SessionInfoCell(CompositeHistoryCell);

impl SessionInfoCell {
    pub(crate) fn new(composite: CompositeHistoryCell) -> Self {
        Self(composite)
    }
}

impl HistoryCell for SessionInfoCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        self.0.display_lines(width)
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.0.desired_height(width)
    }

    fn transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
        self.0.transcript_lines(width)
    }
}

pub(crate) fn new_session_info(
    config: &Config,
    event: SessionConfiguredEvent,
    is_first_event: bool,
) -> SessionInfoCell {
    new_session_info_nori(config, event, is_first_event)
}

pub(crate) fn new_session_info_nori(
    config: &Config,
    event: SessionConfiguredEvent,
    is_first_event: bool,
) -> SessionInfoCell {
    // Use the Nori-branded session header
    crate::nori::session_header::new_nori_session_info(config, event, is_first_event)
}

#[allow(dead_code)]
pub(crate) fn new_session_info_codex(
    config: &Config,
    event: SessionConfiguredEvent,
    is_first_event: bool,
) -> SessionInfoCell {
    let SessionConfiguredEvent {
        model,
        reasoning_effort,
        ..
    } = event;
    SessionInfoCell(if is_first_event {
        // Header box rendered as history (so it appears at the very top)
        let header = SessionHeaderHistoryCell::new(
            model,
            reasoning_effort,
            config.cwd.clone(),
            crate::version::CODEX_CLI_VERSION,
        );

        // Help lines below the header (new copy and list)
        let help_lines: Vec<Line<'static>> = vec![
            "  To get started, describe a task or try one of these commands:"
                .dim()
                .into(),
            Line::from(""),
            Line::from(vec![
                "  ".into(),
                "/init".into(),
                " - create an AGENTS.md file with instructions for Nori".dim(),
            ]),
            Line::from(vec![
                "  ".into(),
                "/status".into(),
                " - show current session configuration".dim(),
            ]),
            Line::from(vec![
                "  ".into(),
                "/approvals".into(),
                " - choose what Nori can do without approval".dim(),
            ]),
            Line::from(vec![
                "  ".into(),
                "/model".into(),
                " - choose what model and reasoning effort to use".dim(),
            ]),
        ];

        CompositeHistoryCell {
            parts: vec![
                Box::new(header),
                Box::new(PlainHistoryCell { lines: help_lines }),
            ],
        }
    } else if config.model == model {
        CompositeHistoryCell { parts: vec![] }
    } else {
        let lines = vec![
            "model changed:".magenta().bold().into(),
            format!("requested: {}", config.model).into(),
            format!("used: {model}").into(),
        ];
        CompositeHistoryCell {
            parts: vec![Box::new(PlainHistoryCell { lines })],
        }
    })
}

pub(crate) fn new_user_prompt(message: String) -> UserHistoryCell {
    UserHistoryCell { message }
}

#[derive(Debug)]
struct SessionHeaderHistoryCell {
    version: &'static str,
    model: String,
    reasoning_effort: Option<ReasoningEffortConfig>,
    directory: PathBuf,
}

impl SessionHeaderHistoryCell {
    fn new(
        model: String,
        reasoning_effort: Option<ReasoningEffortConfig>,
        directory: PathBuf,
        version: &'static str,
    ) -> Self {
        Self {
            version,
            model,
            reasoning_effort,
            directory,
        }
    }

    fn format_directory(&self, max_width: Option<usize>) -> String {
        Self::format_directory_inner(&self.directory, max_width)
    }

    fn format_directory_inner(directory: &Path, max_width: Option<usize>) -> String {
        let formatted = if let Some(rel) = relativize_to_home(directory) {
            if rel.as_os_str().is_empty() {
                "~".to_string()
            } else {
                format!("~{}{}", std::path::MAIN_SEPARATOR, rel.display())
            }
        } else {
            directory.display().to_string()
        };

        if let Some(max_width) = max_width {
            if max_width == 0 {
                return String::new();
            }
            if UnicodeWidthStr::width(formatted.as_str()) > max_width {
                return crate::text_formatting::center_truncate_path(&formatted, max_width);
            }
        }

        formatted
    }

    fn reasoning_label(&self) -> Option<&'static str> {
        self.reasoning_effort.map(|effort| match effort {
            ReasoningEffortConfig::Minimal => "minimal",
            ReasoningEffortConfig::Low => "low",
            ReasoningEffortConfig::Medium => "medium",
            ReasoningEffortConfig::High => "high",
            ReasoningEffortConfig::XHigh => "xhigh",
            ReasoningEffortConfig::None => "none",
        })
    }
}

impl HistoryCell for SessionHeaderHistoryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let Some(inner_width) = card_inner_width(width, SESSION_HEADER_MAX_INNER_WIDTH) else {
            return Vec::new();
        };

        let make_row = |spans: Vec<Span<'static>>| Line::from(spans);

        // Title line rendered inside the box: ">_ Nori (vX)"
        let title_spans: Vec<Span<'static>> = vec![
            Span::from(">_ ").dim(),
            Span::from("Nori").bold(),
            Span::from(" ").dim(),
            Span::from(format!("(v{})", self.version)).dim(),
        ];

        const CHANGE_MODEL_HINT_COMMAND: &str = "/model";
        const CHANGE_MODEL_HINT_EXPLANATION: &str = " to change";
        const DIR_LABEL: &str = "directory:";
        let label_width = DIR_LABEL.len();
        let model_label = format!(
            "{model_label:<label_width$}",
            model_label = "model:",
            label_width = label_width
        );
        let reasoning_label = self.reasoning_label();
        let mut model_spans: Vec<Span<'static>> = vec![
            Span::from(format!("{model_label} ")).dim(),
            Span::from(self.model.clone()),
        ];
        if let Some(reasoning) = reasoning_label {
            model_spans.push(Span::from(" "));
            model_spans.push(Span::from(reasoning));
        }
        model_spans.push("   ".dim());
        model_spans.push(CHANGE_MODEL_HINT_COMMAND.cyan());
        model_spans.push(CHANGE_MODEL_HINT_EXPLANATION.dim());

        let dir_label = format!("{DIR_LABEL:<label_width$}");
        let dir_prefix = format!("{dir_label} ");
        let dir_prefix_width = UnicodeWidthStr::width(dir_prefix.as_str());
        let dir_max_width = inner_width.saturating_sub(dir_prefix_width);
        let dir = self.format_directory(Some(dir_max_width));
        let dir_spans = vec![Span::from(dir_prefix).dim(), Span::from(dir)];

        let lines = vec![
            make_row(title_spans),
            make_row(Vec::new()),
            make_row(model_spans),
            make_row(dir_spans),
        ];

        with_border(lines)
    }
}

#[derive(Debug)]
pub(crate) struct CompositeHistoryCell {
    parts: Vec<Box<dyn HistoryCell>>,
}

impl CompositeHistoryCell {
    pub(crate) fn new(parts: Vec<Box<dyn HistoryCell>>) -> Self {
        Self { parts }
    }
}

impl HistoryCell for CompositeHistoryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut out: Vec<Line<'static>> = Vec::new();
        let mut first = true;
        for part in &self.parts {
            let mut lines = part.display_lines(width);
            if !lines.is_empty() {
                if !first {
                    out.push(Line::from(""));
                }
                out.append(&mut lines);
                first = false;
            }
        }
        out
    }
}

#[derive(Debug)]
pub(crate) struct McpToolCallCell {
    call_id: String,
    invocation: McpInvocation,
    start_time: Instant,
    duration: Option<Duration>,
    result: Option<Result<mcp_types::CallToolResult, String>>,
    animations_enabled: bool,
}

impl McpToolCallCell {
    pub(crate) fn new(
        call_id: String,
        invocation: McpInvocation,
        animations_enabled: bool,
    ) -> Self {
        Self {
            call_id,
            invocation,
            start_time: Instant::now(),
            duration: None,
            result: None,
            animations_enabled,
        }
    }

    pub(crate) fn call_id(&self) -> &str {
        &self.call_id
    }

    pub(crate) fn complete(
        &mut self,
        duration: Duration,
        result: Result<mcp_types::CallToolResult, String>,
    ) -> Option<Box<dyn HistoryCell>> {
        let image_cell = try_new_completed_mcp_tool_call_with_image_output(&result)
            .map(|cell| Box::new(cell) as Box<dyn HistoryCell>);
        self.duration = Some(duration);
        self.result = Some(result);
        image_cell
    }

    fn success(&self) -> Option<bool> {
        match self.result.as_ref() {
            Some(Ok(result)) => Some(!result.is_error.unwrap_or(false)),
            Some(Err(_)) => Some(false),
            None => None,
        }
    }

    pub(crate) fn mark_failed(&mut self) {
        let elapsed = self.start_time.elapsed();
        self.duration = Some(elapsed);
        self.result = Some(Err("interrupted".to_string()));
    }

    fn render_content_block(block: &mcp_types::ContentBlock, width: usize) -> String {
        match block {
            mcp_types::ContentBlock::TextContent(text) => {
                format_and_truncate_tool_result(&text.text, TOOL_CALL_MAX_LINES, width)
            }
            mcp_types::ContentBlock::ImageContent(_) => "<image content>".to_string(),
            mcp_types::ContentBlock::AudioContent(_) => "<audio content>".to_string(),
            mcp_types::ContentBlock::EmbeddedResource(resource) => {
                let uri = match &resource.resource {
                    EmbeddedResourceResource::TextResourceContents(text) => text.uri.clone(),
                    EmbeddedResourceResource::BlobResourceContents(blob) => blob.uri.clone(),
                };
                format!("embedded resource: {uri}")
            }
            mcp_types::ContentBlock::ResourceLink(ResourceLink { uri, .. }) => {
                format!("link: {uri}")
            }
        }
    }
}

impl HistoryCell for McpToolCallCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'static>> = Vec::new();
        let status = self.success();
        let bullet = match status {
            Some(true) => "•".green().bold(),
            Some(false) => "•".red().bold(),
            None => spinner(Some(self.start_time), self.animations_enabled),
        };
        let header_text = if status.is_some() {
            "Called"
        } else {
            "Calling"
        };

        let invocation_line = line_to_static(&format_mcp_invocation(self.invocation.clone()));
        let mut compact_spans = vec![bullet.clone(), " ".into(), header_text.bold(), " ".into()];
        let mut compact_header = Line::from(compact_spans.clone());
        let reserved = compact_header.width();

        let inline_invocation =
            invocation_line.width() <= (width as usize).saturating_sub(reserved);

        if inline_invocation {
            compact_header.extend(invocation_line.spans.clone());
            lines.push(compact_header);
        } else {
            compact_spans.pop(); // drop trailing space for standalone header
            lines.push(Line::from(compact_spans));

            let opts = RtOptions::new((width as usize).saturating_sub(4))
                .initial_indent("".into())
                .subsequent_indent("    ".into());
            let wrapped = word_wrap_line(&invocation_line, opts);
            let body_lines: Vec<Line<'static>> = wrapped.iter().map(line_to_static).collect();
            lines.extend(prefix_lines(body_lines, "  └ ".dim(), "    ".into()));
        }

        let mut detail_lines: Vec<Line<'static>> = Vec::new();
        // Reserve four columns for the tree prefix ("  └ "/"    ") and ensure the wrapper still has at least one cell to work with.
        let detail_wrap_width = (width as usize).saturating_sub(4).max(1);

        if let Some(result) = &self.result {
            match result {
                Ok(mcp_types::CallToolResult { content, .. }) => {
                    if !content.is_empty() {
                        for block in content {
                            let text = Self::render_content_block(block, detail_wrap_width);
                            for segment in text.split('\n') {
                                let line = Line::from(segment.to_string().dim());
                                let wrapped = word_wrap_line(
                                    &line,
                                    RtOptions::new(detail_wrap_width)
                                        .initial_indent("".into())
                                        .subsequent_indent("    ".into()),
                                );
                                detail_lines.extend(wrapped.iter().map(line_to_static));
                            }
                        }
                    }
                }
                Err(err) => {
                    let err_text = format_and_truncate_tool_result(
                        &format!("Error: {err}"),
                        TOOL_CALL_MAX_LINES,
                        width as usize,
                    );
                    let err_line = Line::from(err_text.dim());
                    let wrapped = word_wrap_line(
                        &err_line,
                        RtOptions::new(detail_wrap_width)
                            .initial_indent("".into())
                            .subsequent_indent("    ".into()),
                    );
                    detail_lines.extend(wrapped.iter().map(line_to_static));
                }
            }
        }

        if !detail_lines.is_empty() {
            let initial_prefix: Span<'static> = if inline_invocation {
                "  └ ".dim()
            } else {
                "    ".into()
            };
            lines.extend(prefix_lines(detail_lines, initial_prefix, "    ".into()));
        }

        lines
    }
}

pub(crate) fn new_active_mcp_tool_call(
    call_id: String,
    invocation: McpInvocation,
    animations_enabled: bool,
) -> McpToolCallCell {
    McpToolCallCell::new(call_id, invocation, animations_enabled)
}

pub(crate) fn new_web_search_call(query: String) -> PlainHistoryCell {
    let lines: Vec<Line<'static>> = vec![Line::from(vec![padded_emoji("🌐").into(), query.into()])];
    PlainHistoryCell { lines }
}

/// If the first content is an image, return a new cell with the image.
/// TODO(rgwood-dd): Handle images properly even if they're not the first result.
fn try_new_completed_mcp_tool_call_with_image_output(
    result: &Result<mcp_types::CallToolResult, String>,
) -> Option<CompletedMcpToolCallWithImageOutput> {
    match result {
        Ok(mcp_types::CallToolResult { content, .. }) => {
            if let Some(mcp_types::ContentBlock::ImageContent(image)) = content.first() {
                let raw_data = match base64::engine::general_purpose::STANDARD.decode(&image.data) {
                    Ok(data) => data,
                    Err(e) => {
                        error!("Failed to decode image data: {e}");
                        return None;
                    }
                };
                let reader = match ImageReader::new(Cursor::new(raw_data)).with_guessed_format() {
                    Ok(reader) => reader,
                    Err(e) => {
                        error!("Failed to guess image format: {e}");
                        return None;
                    }
                };

                let image = match reader.decode() {
                    Ok(image) => image,
                    Err(e) => {
                        error!("Image decoding failed: {e}");
                        return None;
                    }
                };

                Some(CompletedMcpToolCallWithImageOutput { _image: image })
            } else {
                None
            }
        }
        _ => None,
    }
}

pub(crate) fn new_skillset_switched_event(name: &str) -> PlainHistoryCell {
    let lines: Vec<Line<'static>> = vec![
        vec![
            "✔ ".green(),
            "Switched to skillset ".green(),
            format!("\"{name}\"").green().bold(),
        ]
        .into(),
    ];
    PlainHistoryCell { lines }
}

#[allow(clippy::disallowed_methods)]
pub(crate) fn new_warning_event(message: String) -> PrefixedWrappedHistoryCell {
    PrefixedWrappedHistoryCell::new(message.yellow(), "⚠ ".yellow(), "  ")
}

#[derive(Debug)]
pub(crate) struct DeprecationNoticeCell {
    summary: String,
    details: Option<String>,
}

pub(crate) fn new_deprecation_notice(
    summary: String,
    details: Option<String>,
) -> DeprecationNoticeCell {
    DeprecationNoticeCell { summary, details }
}

impl HistoryCell for DeprecationNoticeCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'static>> = Vec::new();
        lines.push(vec!["⚠ ".red().bold(), self.summary.clone().red()].into());

        let wrap_width = width.saturating_sub(4).max(1) as usize;

        if let Some(details) = &self.details {
            let line = textwrap::wrap(details, wrap_width)
                .into_iter()
                .map(|s| s.to_string().dim().into())
                .collect::<Vec<_>>();
            lines.extend(line);
        }

        lines
    }
}

/// Render a summary of configured MCP servers from the current `Config`.
pub(crate) fn empty_mcp_output() -> PlainHistoryCell {
    let lines: Vec<Line<'static>> = vec![
        "/mcp".magenta().into(),
        "".into(),
        vec!["🔌  ".into(), "MCP Tools".bold()].into(),
        "".into(),
        "  • No MCP servers configured.".italic().into(),
        Line::from(vec![
            "    See the ".into(),
            "\u{1b}]8;;https://github.com/tilework-tech/nori-cli/blob/main/docs/config.md#mcp_servers\u{7}MCP docs\u{1b}]8;;\u{7}".underlined(),
            " to configure them.".into(),
        ])
        .style(Style::default().add_modifier(Modifier::DIM)),
    ];

    PlainHistoryCell { lines }
}

/// Render MCP tools grouped by connection using the fully-qualified tool names.
pub(crate) fn new_mcp_tools_output(
    config: &Config,
    tools: HashMap<String, mcp_types::Tool>,
    resources: HashMap<String, Vec<Resource>>,
    resource_templates: HashMap<String, Vec<ResourceTemplate>>,
    auth_statuses: &HashMap<String, McpAuthStatus>,
) -> PlainHistoryCell {
    let mut lines: Vec<Line<'static>> = vec![
        "/mcp".magenta().into(),
        "".into(),
        vec!["🔌  ".into(), "MCP Tools".bold()].into(),
        "".into(),
    ];

    if tools.is_empty() {
        lines.push("  • No MCP tools available.".italic().into());
        lines.push("".into());
        return PlainHistoryCell { lines };
    }

    let mut servers: Vec<_> = config.mcp_servers.iter().collect();
    servers.sort_by(|(a, _), (b, _)| a.cmp(b));

    for (server, cfg) in servers {
        let prefix = format!("mcp__{server}__");
        let mut names: Vec<String> = tools
            .keys()
            .filter(|k| k.starts_with(&prefix))
            .map(|k| k[prefix.len()..].to_string())
            .collect();
        names.sort();

        let auth_status = auth_statuses
            .get(server.as_str())
            .copied()
            .unwrap_or(McpAuthStatus::Unsupported);
        let mut header: Vec<Span<'static>> = vec!["  • ".into(), server.clone().into()];
        if !cfg.enabled {
            header.push(" ".into());
            header.push("(disabled)".red());
            lines.push(header.into());
            lines.push(Line::from(""));
            continue;
        }
        lines.push(header.into());
        lines.push(vec!["    • Status: ".into(), "enabled".green()].into());
        lines.push(vec!["    • Auth: ".into(), auth_status.to_string().into()].into());

        match &cfg.transport {
            McpServerTransportConfig::Stdio {
                command,
                args,
                env,
                env_vars,
                cwd,
            } => {
                let args_suffix = if args.is_empty() {
                    String::new()
                } else {
                    format!(" {}", args.join(" "))
                };
                let cmd_display = format!("{command}{args_suffix}");
                lines.push(vec!["    • Command: ".into(), cmd_display.into()].into());

                if let Some(cwd) = cwd.as_ref() {
                    lines.push(vec!["    • Cwd: ".into(), cwd.display().to_string().into()].into());
                }

                let env_display = format_env_display(env.as_ref(), env_vars);
                if env_display != "-" {
                    lines.push(vec!["    • Env: ".into(), env_display.into()].into());
                }
            }
            McpServerTransportConfig::StreamableHttp {
                url,
                http_headers,
                env_http_headers,
                ..
            } => {
                lines.push(vec!["    • URL: ".into(), url.clone().into()].into());
                if let Some(headers) = http_headers.as_ref()
                    && !headers.is_empty()
                {
                    let mut pairs: Vec<_> = headers.iter().collect();
                    pairs.sort_by(|(a, _), (b, _)| a.cmp(b));
                    let display = pairs
                        .into_iter()
                        .map(|(name, _)| format!("{name}=*****"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    lines.push(vec!["    • HTTP headers: ".into(), display.into()].into());
                }
                if let Some(headers) = env_http_headers.as_ref()
                    && !headers.is_empty()
                {
                    let mut pairs: Vec<_> = headers.iter().collect();
                    pairs.sort_by(|(a, _), (b, _)| a.cmp(b));
                    let display = pairs
                        .into_iter()
                        .map(|(name, var)| format!("{name}={var}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    lines.push(vec!["    • Env HTTP headers: ".into(), display.into()].into());
                }
            }
        }

        if names.is_empty() {
            lines.push("    • Tools: (none)".into());
        } else {
            lines.push(vec!["    • Tools: ".into(), names.join(", ").into()].into());
        }

        let server_resources: Vec<Resource> =
            resources.get(server.as_str()).cloned().unwrap_or_default();
        if server_resources.is_empty() {
            lines.push("    • Resources: (none)".into());
        } else {
            let mut spans: Vec<Span<'static>> = vec!["    • Resources: ".into()];

            for (idx, resource) in server_resources.iter().enumerate() {
                if idx > 0 {
                    spans.push(", ".into());
                }

                let label = resource.title.as_ref().unwrap_or(&resource.name);
                spans.push(label.clone().into());
                spans.push(" ".into());
                spans.push(format!("({})", resource.uri).dim());
            }

            lines.push(spans.into());
        }

        let server_templates: Vec<ResourceTemplate> = resource_templates
            .get(server.as_str())
            .cloned()
            .unwrap_or_default();
        if server_templates.is_empty() {
            lines.push("    • Resource templates: (none)".into());
        } else {
            let mut spans: Vec<Span<'static>> = vec!["    • Resource templates: ".into()];

            for (idx, template) in server_templates.iter().enumerate() {
                if idx > 0 {
                    spans.push(", ".into());
                }

                let label = template.title.as_ref().unwrap_or(&template.name);
                spans.push(label.clone().into());
                spans.push(" ".into());
                spans.push(format!("({})", template.uri_template).dim());
            }

            lines.push(spans.into());
        }

        lines.push(Line::from(""));
    }

    PlainHistoryCell { lines }
}
pub(crate) fn new_info_event(message: String, hint: Option<String>) -> PlainHistoryCell {
    let mut line = vec!["• ".dim(), message.into()];
    if let Some(hint) = hint {
        line.push(" ".into());
        line.push(hint.dark_gray());
    }
    let lines: Vec<Line<'static>> = vec![line.into()];
    PlainHistoryCell { lines }
}

pub(crate) fn new_error_event(message: String) -> PlainHistoryCell {
    // Use a hair space (U+200A) to create a subtle, near-invisible separation
    // before the text. VS16 is intentionally omitted to keep spacing tighter
    // in terminals like Ghostty.
    let lines: Vec<Line<'static>> = vec![vec![format!("■ {message}").red()].into()];
    PlainHistoryCell { lines }
}

/// Render plan steps as styled ratatui lines with checkbox formatting.
/// Shared by both `PlanUpdateCell` (history) and `PinnedPlanDrawer` (viewport).
pub(crate) fn render_plan_lines(
    explanation: Option<&str>,
    plan: &[PlanItemArg],
    width: u16,
) -> Vec<Line<'static>> {
    let render_note = |text: &str| -> Vec<Line<'static>> {
        let wrap_width = width.saturating_sub(4).max(1) as usize;
        textwrap::wrap(text, wrap_width)
            .into_iter()
            .map(|s| s.to_string().dim().italic().into())
            .collect()
    };

    let render_step = |status: &StepStatus, text: &str| -> Vec<Line<'static>> {
        let (box_str, step_style) = match status {
            StepStatus::Completed => ("✔ ", Style::default().crossed_out().dim()),
            StepStatus::InProgress => ("□ ", Style::default().cyan().bold()),
            StepStatus::Pending => ("□ ", Style::default().dim()),
        };
        let wrap_width = (width as usize)
            .saturating_sub(4)
            .saturating_sub(box_str.width())
            .max(1);
        let parts = textwrap::wrap(text, wrap_width);
        let step_text = parts
            .into_iter()
            .map(|s| s.to_string().set_style(step_style).into())
            .collect();
        prefix_lines(step_text, box_str.into(), "  ".into())
    };

    let mut lines: Vec<Line<'static>> = vec![];
    lines.push(vec!["• ".dim(), "Updated Plan".bold()].into());

    let mut indented_lines = vec![];
    let note = explanation.map(str::trim).filter(|t| !t.is_empty());
    if let Some(expl) = note {
        indented_lines.extend(render_note(expl));
    };

    if plan.is_empty() {
        indented_lines.push(Line::from("(no steps provided)".dim().italic()));
    } else {
        for PlanItemArg { step, status } in plan.iter() {
            indented_lines.extend(render_step(status, step));
        }
    }
    lines.extend(prefix_lines(indented_lines, "  └ ".dim(), "    ".into()));

    lines
}

/// Render a user‑friendly plan update styled like a checkbox todo list.
pub(crate) fn new_plan_update(update: UpdatePlanArgs) -> PlanUpdateCell {
    let UpdatePlanArgs { explanation, plan } = update;
    PlanUpdateCell { explanation, plan }
}

#[derive(Debug)]
pub(crate) struct PlanUpdateCell {
    explanation: Option<String>,
    plan: Vec<PlanItemArg>,
}

impl HistoryCell for PlanUpdateCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        render_plan_lines(self.explanation.as_deref(), &self.plan, width)
    }
}

/// Create a new `PendingPatch` cell that lists the file‑level summary of
/// a proposed patch. The summary lines should already be formatted (e.g.
/// "A path/to/file.rs").
pub(crate) fn new_patch_event(
    changes: HashMap<PathBuf, FileChange>,
    cwd: &Path,
) -> PatchHistoryCell {
    PatchHistoryCell {
        changes,
        cwd: cwd.to_path_buf(),
    }
}

pub(crate) fn new_patch_apply_failure(stderr: String) -> PlainHistoryCell {
    let mut lines: Vec<Line<'static>> = Vec::new();

    // Failure title
    lines.push(Line::from("✘ Failed to apply patch".magenta().bold()));

    if !stderr.trim().is_empty() {
        let output = output_lines(
            Some(&CommandOutput {
                exit_code: 1,
                formatted_output: String::new(),
                aggregated_output: stderr,
            }),
            OutputLinesParams {
                line_limit: TOOL_CALL_MAX_LINES,
                only_err: true,
                include_angle_pipe: true,
                include_prefix: true,
            },
        );
        lines.extend(output.lines);
    }

    PlainHistoryCell { lines }
}

pub(crate) fn new_view_image_tool_call(path: PathBuf, cwd: &Path) -> PlainHistoryCell {
    let display_path = display_path_for(&path, cwd);

    let lines: Vec<Line<'static>> = vec![
        vec!["• ".dim(), "Viewed Image".bold()].into(),
        vec!["  └ ".dim(), display_path.dim()].into(),
    ];

    PlainHistoryCell { lines }
}

pub(crate) fn new_reasoning_summary_block(
    full_reasoning_buffer: String,
    config: &Config,
) -> Box<dyn HistoryCell> {
    if config.model_family.reasoning_summary_format == ReasoningSummaryFormat::Experimental {
        // Experimental format is following:
        // ** header **
        //
        // reasoning summary
        //
        // So we need to strip header from reasoning summary
        let full_reasoning_buffer = full_reasoning_buffer.trim();
        if let Some(open) = full_reasoning_buffer.find("**") {
            let after_open = &full_reasoning_buffer[(open + 2)..];
            if let Some(close) = after_open.find("**") {
                let after_close_idx = open + 2 + close + 2;
                // if we don't have anything beyond `after_close_idx`
                // then we don't have a summary to inject into history
                if after_close_idx < full_reasoning_buffer.len() {
                    let header_buffer = full_reasoning_buffer[..after_close_idx].to_string();
                    let summary_buffer = full_reasoning_buffer[after_close_idx..].to_string();
                    return Box::new(ReasoningSummaryCell::new(
                        header_buffer,
                        summary_buffer,
                        false,
                    ));
                }
            }
        }
    }
    Box::new(ReasoningSummaryCell::new(
        "".to_string(),
        full_reasoning_buffer,
        true,
    ))
}

#[derive(Debug)]
pub struct FinalMessageSeparator {
    elapsed_seconds: Option<u64>,
}
impl FinalMessageSeparator {
    pub(crate) fn new(elapsed_seconds: Option<u64>) -> Self {
        Self { elapsed_seconds }
    }
}
impl HistoryCell for FinalMessageSeparator {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let elapsed_seconds = self
            .elapsed_seconds
            .map(super::status_indicator_widget::fmt_elapsed_compact);
        if let Some(elapsed_seconds) = elapsed_seconds {
            let worked_for = format!("─ Worked for {elapsed_seconds} ─");
            let worked_for_width = worked_for.width();
            vec![
                Line::from_iter([
                    worked_for,
                    "─".repeat((width as usize).saturating_sub(worked_for_width)),
                ])
                .dim(),
            ]
        } else {
            vec![Line::from_iter(["─".repeat(width as usize).dim()])]
        }
    }
}

fn format_mcp_invocation<'a>(invocation: McpInvocation) -> Line<'a> {
    let args_str = invocation
        .arguments
        .as_ref()
        .map(|v: &serde_json::Value| {
            // Use compact form to keep things short but readable.
            serde_json::to_string(v).unwrap_or_else(|_| v.to_string())
        })
        .unwrap_or_default();

    let invocation_spans = vec![
        invocation.server.clone().cyan(),
        ".".into(),
        invocation.tool.cyan(),
        "(".into(),
        args_str.dim(),
        ")".into(),
    ];
    invocation_spans.into()
}

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use ratatui::prelude::*;
use ratatui::style::Stylize;
use textwrap::WordSplitter;

use crate::client_event_format::format_artifacts;
use crate::client_event_format::format_edit_tool_header;
use crate::client_event_format::format_invocation;
use crate::client_event_format::format_tool_header;
use crate::client_event_format::format_tool_kind;
use crate::client_event_format::is_exploring_snapshot;
use crate::client_event_format::is_invocation_redundant;
use crate::client_event_format::relativize_paths_in_text;
use crate::client_event_format::strip_code_fences;
use crate::diff_render::create_diff_summary;
use crate::exec_cell::OutputLinesParams;
use crate::exec_cell::TOOL_CALL_MAX_LINES;
use crate::exec_cell::limit_lines_from_start;
use crate::exec_cell::output_lines;
use crate::exec_cell::spinner;
use crate::exec_cell::truncate_lines_middle;
use crate::history_cell::HistoryCell;
use crate::render::highlight::highlight_bash_to_lines;
use crate::render::line_utils::prefix_lines;
use crate::render::line_utils::push_owned_lines;
use crate::wrapping::RtOptions;
use crate::wrapping::word_wrap_line;
use crate::wrapping::word_wrap_lines;

#[derive(Debug)]
pub(crate) struct ClientToolCell {
    snapshot: nori_protocol::ToolSnapshot,
    exploring_snapshots: Vec<nori_protocol::ToolSnapshot>,
    cwd: PathBuf,
    edit_changes: HashMap<PathBuf, codex_core::protocol::FileChange>,
    animations_enabled: bool,
    start_time: Option<Instant>,
}

impl ClientToolCell {
    pub(crate) fn new(
        snapshot: nori_protocol::ToolSnapshot,
        cwd: PathBuf,
        animations_enabled: bool,
    ) -> Self {
        let start_time = if is_active_phase(&snapshot.phase) {
            Some(Instant::now())
        } else {
            None
        };
        let edit_changes = changes_from_snapshot(&snapshot, &cwd);
        Self {
            snapshot,
            exploring_snapshots: Vec::new(),
            cwd,
            edit_changes,
            animations_enabled,
            start_time,
        }
    }

    pub(crate) fn call_id(&self) -> &str {
        &self.snapshot.call_id
    }

    pub(crate) fn is_active(&self) -> bool {
        is_active_phase(&self.snapshot.phase)
    }

    pub(crate) fn snapshot_kind(&self) -> &nori_protocol::ToolKind {
        &self.snapshot.kind
    }

    pub(crate) fn is_exploring(&self) -> bool {
        is_exploring_snapshot(&self.snapshot) || !self.exploring_snapshots.is_empty()
    }

    /// Return all call_ids held by this cell's exploring group (for tracking
    /// on flush to prevent re-merging into later cells).
    pub(crate) fn exploring_call_ids(&self) -> Vec<String> {
        self.exploring_snapshots
            .iter()
            .map(|s| s.call_id.clone())
            .collect()
    }

    /// Mark this cell as an exploring cell. The primary snapshot becomes the
    /// first item in the exploring group.
    pub(crate) fn mark_exploring(&mut self) {
        if self.exploring_snapshots.is_empty() {
            self.exploring_snapshots.push(self.snapshot.clone());
        }
    }

    /// Add another exploring snapshot to this cell's group, or update an
    /// existing one if the call_id is already present.
    pub(crate) fn merge_exploring(&mut self, snapshot: nori_protocol::ToolSnapshot) {
        // Update in place if the call_id already exists in the group
        if let Some(existing) = self
            .exploring_snapshots
            .iter_mut()
            .find(|s| s.call_id == snapshot.call_id)
        {
            *existing = snapshot;
            return;
        }
        self.exploring_snapshots.push(snapshot);
    }

    pub(crate) fn apply_snapshot(&mut self, snapshot: nori_protocol::ToolSnapshot) {
        if self.snapshot.call_id != snapshot.call_id {
            return;
        }
        if self.start_time.is_none() && is_active_phase(&snapshot.phase) {
            self.start_time = Some(Instant::now());
        }
        if !is_active_phase(&snapshot.phase) {
            self.start_time = None;
        }
        // Also update the corresponding entry in exploring_snapshots so that
        // tool_call_update events (which carry the real path/query) propagate
        // into the exploring rendering.
        if let Some(existing) = self
            .exploring_snapshots
            .iter_mut()
            .find(|s| s.call_id == snapshot.call_id)
        {
            *existing = snapshot.clone();
        }
        self.edit_changes = changes_from_snapshot(&snapshot, &self.cwd);
        self.snapshot = snapshot;
    }

    pub(crate) fn mark_failed(&mut self) {
        if self.is_active() {
            self.snapshot.phase = nori_protocol::ToolPhase::Failed;
            self.start_time = None;
        }
    }

    fn render_exploring_lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut out: Vec<Line<'static>> = Vec::new();

        // Header: Exploring/Explored with bullet
        out.push(Line::from(vec![
            if self.is_active() {
                spinner(self.start_time, self.animations_enabled)
            } else {
                "\u{2022}".dim() // bullet
            },
            " ".into(),
            if self.is_active() {
                "Exploring".bold()
            } else {
                "Explored".bold()
            },
        ]));

        // Build sub-items from exploring snapshots. When exploring_snapshots
        // is empty (single-snapshot exploring cell), use the primary snapshot.
        let mut sub_items: Vec<Line<'static>> = Vec::new();
        let mut i = 0;
        let single_fallback;
        let snapshots = if self.exploring_snapshots.is_empty() {
            single_fallback = [self.snapshot.clone()];
            &single_fallback[..]
        } else {
            &self.exploring_snapshots[..]
        };
        while i < snapshots.len() {
            let snap = &snapshots[i];

            // Group consecutive reads by filename
            if snap.kind == nori_protocol::ToolKind::Read {
                let mut names: Vec<String> = Vec::new();
                if let Some(nori_protocol::Invocation::Read { path }) = &snap.invocation {
                    names.push(read_display_name(path));
                } else {
                    names.push(relativize_paths_in_text(&snap.title, &self.cwd));
                }
                let mut j = i + 1;
                while j < snapshots.len() && snapshots[j].kind == nori_protocol::ToolKind::Read {
                    let next = &snapshots[j];
                    if let Some(nori_protocol::Invocation::Read { path }) = &next.invocation {
                        names.push(read_display_name(path));
                    } else {
                        names.push(relativize_paths_in_text(&next.title, &self.cwd));
                    }
                    j += 1;
                }
                i = j;

                // Build "Read file1.rs, file2.rs" line
                let mut spans: Vec<Span<'static>> = vec!["Read".cyan(), " ".into()];
                for (idx, path) in names.into_iter().enumerate() {
                    if idx > 0 {
                        spans.push(", ".dim());
                    }
                    spans.push(path.into());
                }
                sub_items.push(Line::from(spans));
                continue;
            }

            // Search sub-item
            if snap.kind == nori_protocol::ToolKind::Search {
                if let Some(nori_protocol::Invocation::Search { query, path }) = &snap.invocation {
                    let mut spans: Vec<Span<'static>> = vec!["Search".cyan(), " ".into()];
                    if let Some(q) = query {
                        spans.push(q.clone().into());
                    }
                    if let Some(p) = path {
                        spans.push(" in ".dim());
                        spans.push(
                            relativize_paths_in_text(&p.display().to_string(), &self.cwd).into(),
                        );
                    }
                    sub_items.push(Line::from(spans));
                } else if let Some(nori_protocol::Invocation::ListFiles { path }) = &snap.invocation
                {
                    let mut spans: Vec<Span<'static>> = vec!["List".cyan(), " ".into()];
                    if let Some(p) = path {
                        spans.push(
                            relativize_paths_in_text(&p.display().to_string(), &self.cwd).into(),
                        );
                    }
                    sub_items.push(Line::from(spans));
                } else {
                    sub_items.push(Line::from(vec![
                        "Search".cyan(),
                        " ".into(),
                        relativize_paths_in_text(&snap.title, &self.cwd).into(),
                    ]));
                }
                i += 1;
                continue;
            }

            // ListFiles invocation (from Read/Search tools classified as exploring)
            if matches!(
                &snap.invocation,
                Some(nori_protocol::Invocation::ListFiles { .. })
            ) {
                if let Some(nori_protocol::Invocation::ListFiles { path }) = &snap.invocation {
                    let mut spans: Vec<Span<'static>> = vec!["List".cyan(), " ".into()];
                    if let Some(p) = path {
                        spans.push(
                            relativize_paths_in_text(&p.display().to_string(), &self.cwd).into(),
                        );
                    }
                    sub_items.push(Line::from(spans));
                }
                i += 1;
                continue;
            }

            // Fallback: generic sub-item. Skip the kind label if the title
            // already starts with it (e.g., kind="List", title="List /path"
            // → show "List /path" not "List List /path").
            let kind_label = format_tool_kind(&snap.kind);
            let title = relativize_paths_in_text(&snap.title, &self.cwd);
            if title.to_lowercase().starts_with(&kind_label.to_lowercase()) {
                sub_items.push(Line::from(vec![title.into()]));
            } else {
                sub_items.push(Line::from(vec![
                    kind_label.to_string().cyan(),
                    " ".into(),
                    title.into(),
                ]));
            }
            i += 1;
        }

        // Apply tree-style prefix (└ for first, spaces for subsequent)
        let wrap_width = width.saturating_sub(4).max(1);
        let wrap_opts =
            RtOptions::new(wrap_width as usize).word_splitter(WordSplitter::NoHyphenation);
        let mut wrapped_items: Vec<Line<'static>> = Vec::new();
        for item in sub_items {
            push_owned_lines(
                &word_wrap_line(&item, wrap_opts.clone()),
                &mut wrapped_items,
            );
        }

        out.extend(prefix_lines(
            wrapped_items,
            "  \u{2514} ".dim(),
            "    ".into(),
        ));

        out
    }

    fn render_edit_lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines = Vec::new();

        // Bullet: green for completed, red for failed, spinner for active
        let bullet = if self.snapshot.phase == nori_protocol::ToolPhase::Completed {
            "•".green().bold()
        } else if self.snapshot.phase == nori_protocol::ToolPhase::Failed {
            "•".red().bold()
        } else {
            spinner(self.start_time, self.animations_enabled)
        };

        // For failed edits: show error text or "(failed)" fallback
        if self.snapshot.phase == nori_protocol::ToolPhase::Failed {
            let header =
                relativize_paths_in_text(&format_edit_tool_header(&self.snapshot), &self.cwd);
            lines.push(Line::from(vec![bullet, " ".into(), header.bold()]));
            if let Some(error_text) = extract_error_text(&self.snapshot) {
                lines.push(Line::from(vec!["  └ ".dim(), error_text.dim()]));
            } else {
                lines.push(Line::from(vec!["  └ ".dim(), "(failed)".dim()]));
            }
            return lines;
        }

        if !self.edit_changes.is_empty() {
            let diff_lines = create_diff_summary(&self.edit_changes, &self.cwd, width as usize);

            if let Some((first, rest)) = diff_lines.split_first() {
                // Replace DiffSummary's dim "• " bullet with our phase-aware
                // bullet, and for Move tools swap the "Edited" verb with "Moved".
                let is_move = self.snapshot.kind == nori_protocol::ToolKind::Move;
                let mut header_spans = vec![bullet, " ".into()];
                for span in &first.spans {
                    if span.content.as_ref() == "• " {
                        continue;
                    }
                    if is_move && span.content.as_ref() == "Edited" {
                        header_spans.push("Moved".bold());
                    } else {
                        header_spans.push(span.clone());
                    }
                }
                lines.push(Line::from(header_spans));
                lines.extend(rest.to_vec());
            }
        } else {
            // No diff data: use the format_edit_tool_header as sole header
            let header =
                relativize_paths_in_text(&format_edit_tool_header(&self.snapshot), &self.cwd);
            lines.push(Line::from(vec![bullet, " ".into(), header.bold()]));
        }

        lines
    }

    fn render_generic_lines(&self) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let bullet = if self.is_active() {
            spinner(self.start_time, self.animations_enabled)
        } else if self.snapshot.phase == nori_protocol::ToolPhase::Failed {
            "•".red().bold()
        } else {
            "•".dim()
        };

        let header = relativize_paths_in_text(&format_tool_header(&self.snapshot), &self.cwd);
        lines.push(Line::from(vec![bullet, " ".into(), header.bold()]));

        let mut details = Vec::new();
        if let Some(invocation) = format_invocation(&self.snapshot.invocation)
            && !is_invocation_redundant(&invocation, &self.snapshot.title)
        {
            details.push(relativize_paths_in_text(&invocation, &self.cwd));
        }
        for artifact in format_artifacts(&self.snapshot.artifacts) {
            if artifact.contains('\n') {
                details.extend(artifact.lines().map(str::to_string));
            } else {
                details.push(artifact);
            }
        }

        let is_failed = self.snapshot.phase == nori_protocol::ToolPhase::Failed;

        // For failed tools with no text artifacts, extract error from raw_output
        if is_failed
            && details.is_empty()
            && let Some(error_text) = extract_error_text(&self.snapshot)
        {
            details.push(error_text);
        }

        if details.is_empty() {
            if is_failed {
                // For failed tools with absolutely no detail, show "(failed)" fallback
                details.push("(failed)".to_string());
            } else {
                // Fallback: show location paths when no other detail lines were produced
                for location in &self.snapshot.locations {
                    details.push(location.path.display().to_string());
                }
            }
        }

        for (idx, detail) in details.into_iter().enumerate() {
            let prefix = if idx == 0 { "  └ " } else { "    " };
            lines.push(Line::from(vec![prefix.dim(), detail.dim()]));
        }

        lines
    }

    fn render_execute_lines(&self, width: u16) -> Vec<Line<'static>> {
        let success = exit_code_success(&self.snapshot);
        let bullet = match success {
            Some(true) => "•".green().bold(),
            Some(false) => "•".red().bold(),
            None => spinner(self.start_time, self.animations_enabled),
        };
        let title = if self.is_active() { "Running" } else { "Ran" };

        let command = match &self.snapshot.invocation {
            Some(nori_protocol::Invocation::Command { command }) => command.clone(),
            _ => self.snapshot.title.clone(),
        };

        let mut header_line =
            Line::from(vec![bullet.clone(), " ".into(), title.bold(), " ".into()]);
        let header_prefix_width = header_line.width();

        let highlighted_lines = highlight_bash_to_lines(&command);

        let continuation_wrap_width = usize::from(width)
            .saturating_sub(CMD_CONTINUATION_PREFIX_WIDTH)
            .max(1);
        let continuation_opts =
            RtOptions::new(continuation_wrap_width).word_splitter(WordSplitter::NoHyphenation);

        let mut continuation_lines: Vec<Line<'static>> = Vec::new();

        if let Some((first, rest)) = highlighted_lines.split_first() {
            let available_first_width = (width as usize).saturating_sub(header_prefix_width).max(1);
            let first_opts =
                RtOptions::new(available_first_width).word_splitter(WordSplitter::NoHyphenation);
            let mut first_wrapped: Vec<Line<'static>> = Vec::new();
            push_owned_lines(&word_wrap_line(first, first_opts), &mut first_wrapped);
            let mut first_wrapped_iter = first_wrapped.into_iter();
            if let Some(first_segment) = first_wrapped_iter.next() {
                header_line.extend(first_segment);
            }
            continuation_lines.extend(first_wrapped_iter);

            for line in rest {
                push_owned_lines(
                    &word_wrap_line(line, continuation_opts.clone()),
                    &mut continuation_lines,
                );
            }
        }

        let mut lines: Vec<Line<'static>> = vec![header_line];

        let continuation_lines = limit_lines_from_start(&continuation_lines, CMD_CONTINUATION_MAX);
        if !continuation_lines.is_empty() {
            lines.extend(prefix_lines(
                continuation_lines,
                Span::from(CMD_CONTINUATION_PREFIX).dim(),
                Span::from(CMD_CONTINUATION_PREFIX).dim(),
            ));
        }

        // Render output
        let output_text = execute_output_text(&self.snapshot);

        if let Some(text) = output_text {
            if text.is_empty() {
                lines.extend(prefix_lines(
                    vec![Line::from("(no output)".dim())],
                    Span::from(OUTPUT_INITIAL_PREFIX).dim(),
                    Span::from(OUTPUT_SUBSEQUENT_PREFIX),
                ));
            } else {
                let output = crate::exec_cell::CommandOutput {
                    exit_code: extract_exit_code(&self.snapshot).unwrap_or_else(|| {
                        if self.snapshot.phase == nori_protocol::ToolPhase::Failed {
                            1
                        } else {
                            0
                        }
                    }),
                    aggregated_output: text,
                    formatted_output: String::new(),
                };
                let raw_output = output_lines(
                    Some(&output),
                    OutputLinesParams {
                        line_limit: TOOL_CALL_MAX_LINES,
                        only_err: false,
                        include_angle_pipe: false,
                        include_prefix: false,
                    },
                );

                let trimmed_output =
                    truncate_lines_middle(&raw_output.lines, OUTPUT_MAX_LINES, raw_output.omitted);

                let output_wrap_width = usize::from(width)
                    .saturating_sub(OUTPUT_PREFIX_WIDTH)
                    .max(1);
                let output_opts =
                    RtOptions::new(output_wrap_width).word_splitter(WordSplitter::NoHyphenation);
                let mut wrapped_output: Vec<Line<'static>> = Vec::new();
                for line in trimmed_output {
                    push_owned_lines(
                        &word_wrap_line(&line, output_opts.clone()),
                        &mut wrapped_output,
                    );
                }

                if !wrapped_output.is_empty() {
                    lines.extend(prefix_lines(
                        wrapped_output,
                        Span::from(OUTPUT_INITIAL_PREFIX).dim(),
                        Span::from(OUTPUT_SUBSEQUENT_PREFIX),
                    ));
                }
            }
        } else if !self.is_active() {
            // Only show "(no output)" for completed/failed commands, not in-progress ones.
            lines.extend(prefix_lines(
                vec![Line::from("(no output)".dim())],
                Span::from(OUTPUT_INITIAL_PREFIX).dim(),
                Span::from(OUTPUT_SUBSEQUENT_PREFIX),
            ));
        }

        lines
    }

    /// Transcript rendering for Execute tools: `$ command` shell-style format,
    /// matching the style used in the upstream Codex ExecCell transcript view.
    fn render_execute_transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
        let command = match &self.snapshot.invocation {
            Some(nori_protocol::Invocation::Command { command }) => command.clone(),
            _ => self.snapshot.title.clone(),
        };

        let highlighted_lines = highlight_bash_to_lines(&command);
        let cmd_display = word_wrap_lines(
            &highlighted_lines,
            RtOptions::new(width as usize)
                .initial_indent("$ ".magenta().into())
                .subsequent_indent("    ".into()),
        );
        let mut lines: Vec<Line<'static>> = cmd_display;

        // Output
        if let Some(text) = execute_output_text(&self.snapshot)
            && !text.is_empty()
        {
            for line_str in text.lines() {
                lines.push(Line::from(format!("    {line_str}")).dim());
            }
        }

        // Exit status
        if !is_active_phase(&self.snapshot.phase) {
            let success = exit_code_success(&self.snapshot);
            let result: Line = match success {
                Some(true) => Line::from("\u{2713}".green().bold()),
                Some(false) => {
                    let code = extract_exit_code(&self.snapshot).unwrap_or(1);
                    Line::from(vec!["\u{2717}".red().bold(), format!(" ({code})").into()])
                }
                None => Line::default(),
            };
            if !result.spans.is_empty() {
                lines.push(result);
            }
        }

        lines
    }
}

// Layout constants matching ExecCell display layout
const CMD_CONTINUATION_PREFIX: &str = "  │ ";
const CMD_CONTINUATION_PREFIX_WIDTH: usize = 4;
const CMD_CONTINUATION_MAX: usize = 2;
const OUTPUT_INITIAL_PREFIX: &str = "  └ ";
const OUTPUT_SUBSEQUENT_PREFIX: &str = "    ";
const OUTPUT_PREFIX_WIDTH: usize = 4;
const OUTPUT_MAX_LINES: usize = 5;

fn exit_code_success(snapshot: &nori_protocol::ToolSnapshot) -> Option<bool> {
    if let Some(exit_code) = extract_exit_code(snapshot) {
        return Some(exit_code == 0);
    }
    match snapshot.phase {
        nori_protocol::ToolPhase::Completed => Some(true),
        nori_protocol::ToolPhase::Failed => Some(false),
        _ => None,
    }
}

fn extract_exit_code(snapshot: &nori_protocol::ToolSnapshot) -> Option<i32> {
    snapshot
        .raw_output
        .as_ref()?
        .get("exit_code")
        .and_then(serde_json::Value::as_i64)
        .map(|c| c as i32)
}

fn execute_output_text(snapshot: &nori_protocol::ToolSnapshot) -> Option<String> {
    // Prefer stdout from raw_output (most accurate)
    if let Some(raw) = &snapshot.raw_output
        && let Some(stdout) = raw.get("stdout").and_then(serde_json::Value::as_str)
    {
        return Some(stdout.to_string());
    }

    // Only fall back to artifact text for completed/failed snapshots.
    // During pending/in-progress, artifact text for execute tools is
    // the agent's description (e.g., "Print current UTC date/time"),
    // not stdout.
    if !is_active_phase(&snapshot.phase) {
        for artifact in &snapshot.artifacts {
            if let nori_protocol::Artifact::Text { text } = artifact
                && !text.is_empty()
            {
                return Some(strip_code_fences(text));
            }
        }
    }

    None
}

fn extract_error_text(snapshot: &nori_protocol::ToolSnapshot) -> Option<String> {
    let raw = snapshot.raw_output.as_ref()?;
    for key in ["error", "stderr", "output"] {
        if let Some(text) = raw.get(key).and_then(serde_json::Value::as_str)
            && !text.is_empty()
        {
            return Some(text.to_string());
        }
    }
    // Check if raw_output is itself a string
    if let Some(s) = raw.as_str()
        && !s.is_empty()
    {
        return Some(s.to_string());
    }
    None
}

pub(crate) fn diff_changes_from_artifacts(
    artifacts: &[nori_protocol::Artifact],
    cwd: &std::path::Path,
) -> std::collections::HashMap<std::path::PathBuf, codex_core::protocol::FileChange> {
    let mut changes = std::collections::HashMap::new();
    for artifact in artifacts {
        if let nori_protocol::Artifact::Diff(change) = artifact {
            let file_change = match &change.old_text {
                None => codex_core::protocol::FileChange::Add {
                    content: change.new_text.clone(),
                },
                Some(old_text) => codex_core::protocol::FileChange::Update {
                    unified_diff: create_contextual_patch(
                        &change.path,
                        cwd,
                        old_text,
                        &change.new_text,
                    ),
                    move_path: None,
                },
            };
            changes.insert(change.path.clone(), file_change);
        }
    }
    changes
}

pub(crate) fn changes_from_invocation(
    invocation: &Option<nori_protocol::Invocation>,
    cwd: &std::path::Path,
) -> std::collections::HashMap<std::path::PathBuf, codex_core::protocol::FileChange> {
    let mut changes = std::collections::HashMap::new();
    match invocation.as_ref() {
        Some(nori_protocol::Invocation::FileChanges { changes: fc }) => {
            for change in fc {
                let file_change = match &change.old_text {
                    None => codex_core::protocol::FileChange::Add {
                        content: change.new_text.clone(),
                    },
                    Some(old_text) => codex_core::protocol::FileChange::Update {
                        unified_diff: create_contextual_patch(
                            &change.path,
                            cwd,
                            old_text,
                            &change.new_text,
                        ),
                        move_path: None,
                    },
                };
                changes.insert(change.path.clone(), file_change);
            }
        }
        Some(nori_protocol::Invocation::FileOperations { operations }) => {
            for op in operations {
                let (path, file_change) = match op {
                    nori_protocol::FileOperation::Create { path, new_text } => (
                        path.clone(),
                        codex_core::protocol::FileChange::Add {
                            content: new_text.clone(),
                        },
                    ),
                    nori_protocol::FileOperation::Update {
                        path,
                        old_text,
                        new_text,
                    } => (
                        path.clone(),
                        codex_core::protocol::FileChange::Update {
                            unified_diff: create_contextual_patch(path, cwd, old_text, new_text),
                            move_path: None,
                        },
                    ),
                    nori_protocol::FileOperation::Delete { path, old_text } => (
                        path.clone(),
                        codex_core::protocol::FileChange::Delete {
                            content: old_text.clone().unwrap_or_default(),
                        },
                    ),
                    nori_protocol::FileOperation::Move {
                        from_path,
                        to_path,
                        old_text,
                        new_text,
                    } => {
                        let old = old_text.clone().unwrap_or_default();
                        let new = new_text.clone().unwrap_or_else(|| old.clone());
                        (
                            from_path.clone(),
                            codex_core::protocol::FileChange::Update {
                                unified_diff: create_contextual_patch(from_path, cwd, &old, &new),
                                move_path: Some(to_path.clone()),
                            },
                        )
                    }
                };
                changes.insert(path, file_change);
            }
        }
        _ => {}
    }
    changes
}

fn changes_from_snapshot(
    snapshot: &nori_protocol::ToolSnapshot,
    cwd: &std::path::Path,
) -> HashMap<PathBuf, codex_core::protocol::FileChange> {
    let diff_changes = diff_changes_from_artifacts(&snapshot.artifacts, cwd);
    if diff_changes.is_empty() {
        changes_from_invocation(&snapshot.invocation, cwd)
    } else {
        diff_changes
    }
}

fn create_contextual_patch(
    path: &std::path::Path,
    cwd: &std::path::Path,
    old_text: &str,
    new_text: &str,
) -> String {
    codex_core::util::create_patch_with_context(path, cwd, old_text, new_text)
}

impl HistoryCell for ClientToolCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        if !self.exploring_snapshots.is_empty() || is_exploring_snapshot(&self.snapshot) {
            self.render_exploring_lines(width)
        } else if self.snapshot.kind == nori_protocol::ToolKind::Execute {
            self.render_execute_lines(width)
        } else if matches!(
            self.snapshot.kind,
            nori_protocol::ToolKind::Create
                | nori_protocol::ToolKind::Edit
                | nori_protocol::ToolKind::Delete
                | nori_protocol::ToolKind::Move
        ) {
            self.render_edit_lines(width)
        } else {
            self.render_generic_lines()
        }
    }

    fn transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
        if !self.exploring_snapshots.is_empty() || is_exploring_snapshot(&self.snapshot) {
            self.render_exploring_lines(width)
        } else if self.snapshot.kind == nori_protocol::ToolKind::Execute {
            self.render_execute_transcript_lines(width)
        } else if matches!(
            self.snapshot.kind,
            nori_protocol::ToolKind::Create
                | nori_protocol::ToolKind::Edit
                | nori_protocol::ToolKind::Delete
                | nori_protocol::ToolKind::Move
        ) {
            self.render_edit_lines(width)
        } else {
            self.render_generic_lines()
        }
    }
}

/// Extract a display name for a Read path: use the file name (basename) when
/// available, falling back to the full path display. This matches the Codex
/// ExecCell exploring style where Read items show just the filename.
fn read_display_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

fn is_active_phase(phase: &nori_protocol::ToolPhase) -> bool {
    matches!(
        phase,
        nori_protocol::ToolPhase::Pending
            | nori_protocol::ToolPhase::PendingApproval
            | nori_protocol::ToolPhase::InProgress
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use nori_protocol::Artifact;
    use nori_protocol::Invocation;
    use nori_protocol::ToolKind;
    use nori_protocol::ToolPhase;
    use nori_protocol::ToolSnapshot;

    fn render_lines(lines: &[Line<'static>]) -> Vec<String> {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect()
    }

    fn write_updated_rust_fixture(
        dir: &std::path::Path,
        relative_path: &str,
        changed_line: i32,
        changed_text: &str,
    ) -> PathBuf {
        let file_path = dir.join(relative_path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).expect("create fixture parent dir");
        }
        let file_content = (1..=100)
            .map(|line| {
                if line == changed_line {
                    format!("{changed_text}\n")
                } else {
                    format!("fn line_{line}() {{}}\n")
                }
            })
            .collect::<String>();
        std::fs::write(&file_path, file_content).expect("write fixture file");
        file_path
    }

    fn make_execute_snapshot(
        phase: ToolPhase,
        command: &str,
        artifacts: Vec<Artifact>,
        raw_output: Option<serde_json::Value>,
    ) -> ToolSnapshot {
        ToolSnapshot {
            call_id: "call-1".into(),
            title: command.into(),
            kind: ToolKind::Execute,
            phase,
            locations: vec![],
            invocation: Some(Invocation::Command {
                command: command.into(),
            }),
            artifacts,
            raw_input: Some(serde_json::json!({"command": command})),
            raw_output,
            owner_request_id: None,
        }
    }

    #[test]
    fn execute_completed_with_output_shows_ran_and_output() {
        let snapshot = make_execute_snapshot(
            ToolPhase::Completed,
            "date --utc",
            vec![Artifact::Text {
                text: "2026-03-30 05:45:34 UTC".into(),
            }],
            Some(serde_json::json!({"exit_code": 0, "stdout": "2026-03-30 05:45:34 UTC"})),
        );
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/test-cwd"), false);
        let lines = render_lines(&cell.display_lines(80));

        // First line: green bullet + "Ran" + command
        assert!(lines[0].contains("Ran"));
        assert!(lines[0].contains("date --utc"));
        // Should NOT have the generic "Tool [completed]" format
        assert!(!lines[0].contains("Tool ["));

        // Output line under └ prefix
        assert!(lines.iter().any(|l| l.contains("2026-03-30 05:45:34 UTC")));
    }

    #[test]
    fn execute_completed_no_output_shows_no_output() {
        let snapshot = make_execute_snapshot(
            ToolPhase::Completed,
            "rm /tmp/foo.txt",
            vec![],
            Some(serde_json::json!({"exit_code": 0, "stdout": ""})),
        );
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/test-cwd"), false);
        let lines = render_lines(&cell.display_lines(80));

        assert!(lines[0].contains("Ran"));
        assert!(lines[0].contains("rm /tmp/foo.txt"));
        assert!(lines.iter().any(|l| l.contains("(no output)")));
    }

    #[test]
    fn execute_failed_shows_red_bullet_text() {
        let snapshot = make_execute_snapshot(
            ToolPhase::Failed,
            "cargo test",
            vec![Artifact::Text {
                text: "error[E0308]: mismatched types".into(),
            }],
            Some(serde_json::json!({"exit_code": 1, "stdout": "error[E0308]: mismatched types"})),
        );
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/test-cwd"), false);
        let lines = cell.display_lines(80);

        // Verify the bullet span is red+bold (not dim)
        let bullet_span = &lines[0].spans[0];
        assert!(
            bullet_span.style.fg == Some(ratatui::style::Color::Red),
            "Failed execute should have red bullet, got {:?}",
            bullet_span.style
        );

        let text_lines = render_lines(&lines);
        assert!(text_lines[0].contains("Ran"));
    }

    #[test]
    fn execute_in_progress_shows_running() {
        let snapshot = make_execute_snapshot(ToolPhase::InProgress, "cargo build", vec![], None);
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/test-cwd"), false);
        let lines = render_lines(&cell.display_lines(80));

        assert!(lines[0].contains("Running"));
        assert!(lines[0].contains("cargo build"));
        assert!(!lines[0].contains("Tool ["));
    }

    #[test]
    fn execute_output_truncation_with_many_lines() {
        let long_output = (1..=20)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let snapshot = make_execute_snapshot(
            ToolPhase::Completed,
            "cat big_file.txt",
            vec![Artifact::Text {
                text: long_output.clone(),
            }],
            Some(serde_json::json!({"exit_code": 0, "stdout": long_output})),
        );
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/test-cwd"), false);
        let lines = render_lines(&cell.display_lines(80));

        // Should have truncation indicator
        assert!(
            lines.iter().any(|l| l.contains("…") && l.contains("lines")),
            "Expected truncation ellipsis in output, got: {lines:?}"
        );

        // Should NOT show all 20 lines
        let output_lines: Vec<_> = lines.iter().filter(|l| l.contains("line ")).collect();
        assert!(
            output_lines.len() < 20,
            "Expected truncation but got {count} output lines",
            count = output_lines.len()
        );
    }

    #[test]
    fn non_execute_non_exploring_tool_uses_generic_rendering() {
        // Fetch is not an exploring tool kind, so it should use generic rendering
        let snapshot = ToolSnapshot {
            call_id: "call-2".into(),
            title: "Fetch https://example.com".into(),
            kind: ToolKind::Fetch,
            phase: ToolPhase::Completed,
            locations: vec![],
            invocation: Some(Invocation::Tool {
                tool_name: "fetch".into(),
                input: None,
            }),
            artifacts: vec![Artifact::Text {
                text: "200 OK".into(),
            }],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        };
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/test-cwd"), false);
        let lines = render_lines(&cell.display_lines(80));

        // Should use the generic format
        assert!(
            lines[0].contains("Tool [completed]"),
            "Non-execute, non-exploring tool should use generic format, got: {}",
            lines[0]
        );
    }

    #[test]
    fn execute_strips_code_fences_from_artifact_text() {
        let snapshot = make_execute_snapshot(
            ToolPhase::Completed,
            "uptime -p",
            vec![Artifact::Text {
                text: "```console\nup 1 day, 20 hours\n```".into(),
            }],
            Some(serde_json::json!({"exit_code": 0, "stdout": "up 1 day, 20 hours"})),
        );
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/test-cwd"), false);
        let lines = render_lines(&cell.display_lines(80));

        // Should NOT show ```console or ``` literals
        assert!(
            !lines.iter().any(|l| l.contains("```")),
            "Code fences should be stripped, got: {lines:?}"
        );
        // Should show the actual output
        assert!(lines.iter().any(|l| l.contains("up 1 day, 20 hours")));
    }

    #[test]
    fn execute_success_has_green_bullet() {
        let snapshot = make_execute_snapshot(
            ToolPhase::Completed,
            "echo ok",
            vec![Artifact::Text { text: "ok".into() }],
            Some(serde_json::json!({"exit_code": 0, "stdout": "ok"})),
        );
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/test-cwd"), false);
        let lines = cell.display_lines(80);

        let bullet_span = &lines[0].spans[0];
        assert!(
            bullet_span.style.fg == Some(ratatui::style::Color::Green),
            "Successful execute should have green bullet, got {:?}",
            bullet_span.style
        );
    }

    #[test]
    fn execute_fallback_to_title_when_no_invocation() {
        let snapshot = ToolSnapshot {
            call_id: "call-1".into(),
            title: "ls -la".into(),
            kind: ToolKind::Execute,
            phase: ToolPhase::Completed,
            locations: vec![],
            invocation: None,
            artifacts: vec![],
            raw_input: None,
            raw_output: Some(serde_json::json!({"exit_code": 0, "stdout": ""})),
            owner_request_id: None,
        };
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/test-cwd"), false);
        let lines = render_lines(&cell.display_lines(80));

        assert!(lines[0].contains("Ran"));
        assert!(lines[0].contains("ls -la"));
    }

    // --- Spec 08: Gemini Empty Content Fallback ---

    #[test]
    fn generic_completed_with_no_details_but_locations_shows_location_paths() {
        let snapshot = ToolSnapshot {
            call_id: "call-3".into(),
            title: "Fetch resource".into(),
            kind: ToolKind::Fetch,
            phase: ToolPhase::Completed,
            locations: vec![nori_protocol::ToolLocation {
                path: "/repo/README.md".into(),
                line: None,
            }],
            invocation: None,
            artifacts: vec![],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        };
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/test-cwd"), false);
        let lines = render_lines(&cell.display_lines(80));

        // Should have more than just the header line
        assert!(
            lines.len() > 1,
            "Expected location detail lines, got only: {lines:?}"
        );
        // Location path should appear in the detail lines
        assert!(
            lines.iter().any(|l| l.contains("/repo/README.md")),
            "Expected location path in output, got: {lines:?}"
        );
    }

    #[test]
    fn generic_completed_with_no_details_no_locations_still_renders_header() {
        let snapshot = ToolSnapshot {
            call_id: "call-4".into(),
            title: "Some tool".into(),
            kind: ToolKind::Fetch,
            phase: ToolPhase::Completed,
            locations: vec![],
            invocation: None,
            artifacts: vec![],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        };
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/test-cwd"), false);
        let lines = render_lines(&cell.display_lines(80));

        // Should still render at least a header line
        assert!(!lines.is_empty(), "Expected at least a header line");
        assert!(
            lines[0].contains("Some tool"),
            "Header should contain the title, got: {}",
            lines[0]
        );
    }

    // --- Spec 06: Artifact Text Output Cleanup ---

    #[test]
    fn generic_tool_strips_code_fences_from_text_artifact() {
        let snapshot = ToolSnapshot {
            call_id: "call-fence".into(),
            title: "Fetch https://example.com/main.rs".into(),
            kind: ToolKind::Fetch,
            phase: ToolPhase::Completed,
            locations: vec![],
            invocation: Some(Invocation::Tool {
                tool_name: "fetch".into(),
                input: None,
            }),
            artifacts: vec![Artifact::Text {
                text: "```rust\nfn main() {}\n```".into(),
            }],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        };
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/test-cwd"), false);
        let lines = render_lines(&cell.display_lines(80));

        // Code fence markers should NOT appear in the rendered output
        assert!(
            !lines.iter().any(|l| l.contains("```")),
            "Code fences should be stripped from generic tool output, got: {lines:?}"
        );
        // The actual content should still appear
        assert!(
            lines.iter().any(|l| l.contains("fn main() {}")),
            "Content inside fences should still render, got: {lines:?}"
        );
    }

    #[test]
    fn generic_tool_single_line_output_has_no_output_prefix() {
        let snapshot = ToolSnapshot {
            call_id: "call-single".into(),
            title: "Fetch https://example.com".into(),
            kind: ToolKind::Fetch,
            phase: ToolPhase::Completed,
            locations: vec![],
            invocation: None,
            artifacts: vec![Artifact::Text {
                text: "200 OK".into(),
            }],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        };
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/test-cwd"), false);
        let lines = render_lines(&cell.display_lines(80));

        // Should NOT have "Output:" prefix for single-line output
        assert!(
            !lines.iter().any(|l| l.contains("Output:")),
            "Single-line output should not have 'Output:' prefix, got: {lines:?}"
        );
        // The actual text should still appear
        assert!(
            lines.iter().any(|l| l.contains("200 OK")),
            "Output text should still render, got: {lines:?}"
        );
    }

    #[test]
    fn generic_tool_multi_line_output_has_no_output_prefix() {
        let snapshot = ToolSnapshot {
            call_id: "call-multi".into(),
            title: "Think about pattern".into(),
            kind: ToolKind::Think,
            phase: ToolPhase::Completed,
            locations: vec![],
            invocation: Some(Invocation::Tool {
                tool_name: "think".into(),
                input: None,
            }),
            artifacts: vec![Artifact::Text {
                text: "line one\nline two\nline three".into(),
            }],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        };
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/test-cwd"), false);
        let lines = render_lines(&cell.display_lines(80));

        // Should NOT have "Output:" prefix for multi-line output either
        assert!(
            !lines.iter().any(|l| l.contains("Output:")),
            "Multi-line output should not have 'Output:' prefix, got: {lines:?}"
        );
        // Content lines should still appear
        assert!(
            lines.iter().any(|l| l.contains("line one")),
            "First content line should render, got: {lines:?}"
        );
    }

    #[test]
    fn generic_tool_invocation_omitted_when_redundant_with_title() {
        let snapshot = ToolSnapshot {
            call_id: "call-dup".into(),
            title: "Fetch https://example.com".into(),
            kind: ToolKind::Fetch,
            phase: ToolPhase::Completed,
            locations: vec![],
            invocation: Some(Invocation::Tool {
                tool_name: "Fetch https://example.com".into(),
                input: None,
            }),
            artifacts: vec![Artifact::Text {
                text: "# README".into(),
            }],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        };
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/test-cwd"), false);
        let lines = render_lines(&cell.display_lines(80));

        // The invocation detail line should NOT appear because the title
        // already contains the same information
        assert!(
            !lines.iter().any(|l| l.contains("Tool: Fetch")),
            "Redundant invocation should be omitted when title contains same info, got: {lines:?}"
        );
        // But the artifact output should still appear
        assert!(
            lines.iter().any(|l| l.contains("# README")),
            "Output content should still render, got: {lines:?}"
        );
    }

    // --- Spec 04: Path Display Normalization ---

    #[test]
    fn generic_tool_title_relativizes_cwd_paths() {
        let cwd = PathBuf::from("/home/user/project");
        let snapshot = ToolSnapshot {
            call_id: "call-path".into(),
            title: "Fetch /home/user/project/src/main.rs".into(),
            kind: ToolKind::Fetch,
            phase: ToolPhase::Completed,
            locations: vec![],
            invocation: Some(Invocation::Tool {
                tool_name: "fetch".into(),
                input: None,
            }),
            artifacts: vec![Artifact::Text {
                text: "fn main() {}".into(),
            }],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        };
        let cell = ClientToolCell::new(snapshot, cwd, false);
        let lines = render_lines(&cell.display_lines(80));

        // The absolute path should NOT appear in the header
        assert!(
            !lines[0].contains("/home/user/project/src/main.rs"),
            "Absolute path should be relativized in title, got: {}",
            lines[0]
        );
        // The relative path should appear instead
        assert!(
            lines[0].contains("src/main.rs"),
            "Relative path should appear in title, got: {}",
            lines[0]
        );
    }

    #[test]
    fn generic_tool_invocation_path_relativized() {
        let cwd = PathBuf::from("/home/user/project");
        let snapshot = ToolSnapshot {
            call_id: "call-inv-path".into(),
            title: "Search in /home/user/project/src".into(),
            kind: ToolKind::Search,
            phase: ToolPhase::Completed,
            locations: vec![],
            invocation: Some(Invocation::Search {
                query: Some("TODO".into()),
                path: Some("/home/user/project/src".into()),
            }),
            artifacts: vec![Artifact::Text {
                text: "found 3 results".into(),
            }],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        };
        let cell = ClientToolCell::new(snapshot, cwd, false);
        let lines = render_lines(&cell.display_lines(80));

        // Invocation detail should NOT have absolute path
        let detail_lines: Vec<_> = lines.iter().filter(|l| l.contains("Search")).collect();
        assert!(
            !detail_lines
                .iter()
                .any(|l| l.contains("/home/user/project/src")),
            "Absolute path should be relativized in invocation detail, got: {detail_lines:?}"
        );
    }

    #[test]
    fn execute_tool_command_not_path_mangled() {
        let cwd = PathBuf::from("/home/user/project");
        let snapshot = make_execute_snapshot(
            ToolPhase::Completed,
            "cat /home/user/project/README.md",
            vec![Artifact::Text {
                text: "# Readme".into(),
            }],
            Some(serde_json::json!({"exit_code": 0, "stdout": "# Readme"})),
        );
        // Override the default cwd for this test
        let cell = ClientToolCell::new(snapshot, cwd, false);
        let lines = render_lines(&cell.display_lines(80));

        // Execute commands should keep their original command text
        // (path normalization in the command itself is NOT desired for execute tools)
        assert!(
            lines[0].contains("Ran"),
            "Execute should still show 'Ran', got: {}",
            lines[0]
        );
    }

    // --- Spec 07: Diff Artifact Rendering in ClientToolCell ---

    #[test]
    fn generic_tool_renders_diff_artifacts() {
        let snapshot = ToolSnapshot {
            call_id: "call-diff".into(),
            title: "Edit README.md".into(),
            kind: ToolKind::Edit,
            phase: ToolPhase::InProgress,
            locations: vec![nori_protocol::ToolLocation {
                path: PathBuf::from("README.md"),
                line: None,
            }],
            invocation: None,
            artifacts: vec![nori_protocol::Artifact::Diff(nori_protocol::FileChange {
                path: PathBuf::from("README.md"),
                old_text: Some("# Old Title\n".into()),
                new_text: "# New Title\n".into(),
            })],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        };
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/test-cwd"), false);
        let lines = render_lines(&cell.display_lines(80));

        // Should render some diff content -- at minimum the file path
        assert!(
            lines.iter().any(|l| l.contains("README.md")),
            "Diff artifact should render file path in output, got: {lines:?}"
        );
        // Should show some diff indicator (add/remove line counts or content)
        let has_diff_indicator = lines.iter().any(|l| {
            l.contains('+') || l.contains('-') || l.contains("Old Title") || l.contains("New Title")
        });
        assert!(
            has_diff_indicator,
            "Diff artifact should render diff content, got: {lines:?}"
        );
    }

    // --- Spec 02: Exploring Cell Grouping ---

    #[test]
    fn exploring_cell_with_single_read_shows_explored_header() {
        let snapshot = ToolSnapshot {
            call_id: "call-read-1".into(),
            title: "Read README.md".into(),
            kind: ToolKind::Read,
            phase: ToolPhase::Completed,
            locations: vec![nori_protocol::ToolLocation {
                path: PathBuf::from("README.md"),
                line: None,
            }],
            invocation: Some(Invocation::Read {
                path: "README.md".into(),
            }),
            artifacts: vec![Artifact::Text {
                text: "# Title\nSome content".into(),
            }],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        };
        let mut cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/cwd"), false);
        cell.mark_exploring();
        let lines = render_lines(&cell.display_lines(80));

        // Should show "Explored" header, not "Tool [completed]"
        assert!(
            lines[0].contains("Explored"),
            "Exploring cell should show 'Explored' header, got: {}",
            lines[0]
        );

        // Should show the read file in a sub-item
        assert!(
            lines
                .iter()
                .any(|l| l.contains("Read") && l.contains("README.md")),
            "Exploring cell should show Read sub-item, got: {lines:?}"
        );

        // Should NOT show the read content (output is noise in exploring)
        assert!(
            !lines
                .iter()
                .any(|l| l.contains("# Title") || l.contains("Some content")),
            "Exploring cell should omit read output content, got: {lines:?}"
        );
    }

    #[test]
    fn exploring_cell_groups_multiple_reads() {
        let snap1 = ToolSnapshot {
            call_id: "call-r1".into(),
            title: "Read file1.rs".into(),
            kind: ToolKind::Read,
            phase: ToolPhase::Completed,
            locations: vec![],
            invocation: Some(Invocation::Read {
                path: "file1.rs".into(),
            }),
            artifacts: vec![],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        };
        let snap2 = ToolSnapshot {
            call_id: "call-r2".into(),
            title: "Read file2.rs".into(),
            kind: ToolKind::Read,
            phase: ToolPhase::Completed,
            locations: vec![],
            invocation: Some(Invocation::Read {
                path: "file2.rs".into(),
            }),
            artifacts: vec![],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        };

        let mut cell = ClientToolCell::new(snap1, PathBuf::from("/tmp/cwd"), false);
        cell.mark_exploring();
        cell.merge_exploring(snap2);
        let lines = render_lines(&cell.display_lines(80));

        // Should group reads on a single line: "Read file1.rs, file2.rs"
        assert!(
            lines
                .iter()
                .any(|l| l.contains("file1.rs") && l.contains("file2.rs")),
            "Exploring cell should group reads, got: {lines:?}"
        );
    }

    #[test]
    fn exploring_cell_shows_search_sub_item() {
        let snap = ToolSnapshot {
            call_id: "call-search".into(),
            title: "Search TODO".into(),
            kind: ToolKind::Search,
            phase: ToolPhase::Completed,
            locations: vec![],
            invocation: Some(Invocation::Search {
                query: Some("TODO".into()),
                path: Some("/repo/src".into()),
            }),
            artifacts: vec![],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        };

        let mut cell = ClientToolCell::new(snap, PathBuf::from("/tmp/cwd"), false);
        cell.mark_exploring();
        let lines = render_lines(&cell.display_lines(80));

        assert!(
            lines
                .iter()
                .any(|l| l.contains("Search") && l.contains("TODO")),
            "Exploring cell should show Search sub-item, got: {lines:?}"
        );
    }

    #[test]
    fn exploring_cell_in_progress_shows_exploring_header() {
        let snapshot = ToolSnapshot {
            call_id: "call-read-active".into(),
            title: "Read file.rs".into(),
            kind: ToolKind::Read,
            phase: ToolPhase::InProgress,
            locations: vec![],
            invocation: Some(Invocation::Read {
                path: "file.rs".into(),
            }),
            artifacts: vec![],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        };
        let mut cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/cwd"), false);
        cell.mark_exploring();
        let lines = render_lines(&cell.display_lines(80));

        // Should show "Exploring" (active), not "Explored"
        assert!(
            lines[0].contains("Exploring"),
            "Active exploring cell should show 'Exploring', got: {}",
            lines[0]
        );
    }

    // --- Spec 12: Execute Cell Completion Buffering ---

    #[test]
    fn execute_in_progress_with_description_artifact_shows_no_output() {
        // Claude sends the tool description in the content array of in-progress
        // execute updates. This description text should NOT be rendered as stdout.
        let snapshot = make_execute_snapshot(
            ToolPhase::InProgress,
            "date --utc +\"%Y-%m-%d %H:%M:%S %Z\"",
            vec![Artifact::Text {
                text: "Print current UTC date/time with format flags".into(),
            }],
            None, // no raw_output yet (in-progress)
        );
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/test-cwd"), false);
        let lines = render_lines(&cell.display_lines(80));

        // Should show "Running" header with command
        assert!(
            lines[0].contains("Running"),
            "In-progress execute should show 'Running', got: {}",
            lines[0]
        );
        // Should NOT show the description text as output
        assert!(
            !lines.iter().any(|l| l.contains("Print current UTC")),
            "Description text should not be rendered as execute output, got: {lines:?}"
        );
    }

    #[test]
    fn list_files_title_not_duplicated_in_exploring_fallback() {
        // When the kind maps to a label that already prefixes the title,
        // the exploring renderer should not duplicate it.
        let snapshot = ToolSnapshot {
            call_id: "call-list-dup".into(),
            title: "List /home/user/project/src".into(),
            kind: ToolKind::Other("List".into()),
            phase: ToolPhase::Completed,
            locations: vec![],
            invocation: None, // no ListFiles invocation, so hits generic fallback
            artifacts: vec![],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        };
        let mut cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/cwd"), false);
        cell.mark_exploring();
        let lines = render_lines(&cell.display_lines(80));

        // Should NOT show "List List /home/..."
        let has_double_list = lines.iter().any(|l| l.contains("List List"));
        assert!(
            !has_double_list,
            "Should not duplicate 'List' label, got: {lines:?}"
        );
        // Should still show the path
        assert!(
            lines.iter().any(|l| l.contains("/home/user/project/src")),
            "Should still show the path, got: {lines:?}"
        );
    }

    // --- Spec 10: Failed Edit Tool Visibility ---

    fn make_edit_snapshot(
        phase: ToolPhase,
        path: &str,
        artifacts: Vec<Artifact>,
        raw_output: Option<serde_json::Value>,
    ) -> ToolSnapshot {
        ToolSnapshot {
            call_id: "call-edit-1".into(),
            title: format!("Edit {path}"),
            kind: ToolKind::Edit,
            phase,
            locations: vec![nori_protocol::ToolLocation {
                path: PathBuf::from(path),
                line: None,
            }],
            invocation: None,
            artifacts,
            raw_input: None,
            raw_output,
            owner_request_id: None,
        }
    }

    #[test]
    fn failed_edit_has_red_bullet() {
        let snapshot = make_edit_snapshot(ToolPhase::Failed, "README.md", vec![], None);
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/test-cwd"), false);
        let lines = cell.display_lines(80);

        let bullet_span = &lines[0].spans[0];
        assert!(
            bullet_span.style.fg == Some(ratatui::style::Color::Red),
            "Failed edit should have red bullet, got {:?}",
            bullet_span.style
        );
    }

    #[test]
    fn failed_edit_has_semantic_header() {
        let snapshot = make_edit_snapshot(ToolPhase::Failed, "README.md", vec![], None);
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/test-cwd"), false);
        let lines = render_lines(&cell.display_lines(80));

        assert!(
            lines[0].contains("Edit failed:"),
            "Failed edit should show 'Edit failed:' header, got: {}",
            lines[0]
        );
        assert!(
            lines[0].contains("README.md"),
            "Failed edit header should include path, got: {}",
            lines[0]
        );
        assert!(
            !lines[0].contains("Tool ["),
            "Failed edit should NOT use generic 'Tool [failed]' header, got: {}",
            lines[0]
        );
    }

    #[test]
    fn failed_delete_has_semantic_header() {
        let snapshot = ToolSnapshot {
            call_id: "call-del-1".into(),
            title: "Delete temp.txt".into(),
            kind: ToolKind::Delete,
            phase: ToolPhase::Failed,
            locations: vec![nori_protocol::ToolLocation {
                path: PathBuf::from("temp.txt"),
                line: None,
            }],
            invocation: None,
            artifacts: vec![],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        };
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/test-cwd"), false);
        let lines = render_lines(&cell.display_lines(80));

        assert!(
            lines[0].contains("Delete failed:"),
            "Failed delete should show 'Delete failed:' header, got: {}",
            lines[0]
        );
        assert!(
            lines[0].contains("temp.txt"),
            "Failed delete header should include path, got: {}",
            lines[0]
        );
    }

    #[test]
    fn in_progress_edit_has_semantic_header() {
        let snapshot = make_edit_snapshot(ToolPhase::InProgress, "src/main.rs", vec![], None);
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/test-cwd"), false);
        let lines = render_lines(&cell.display_lines(80));

        assert!(
            lines[0].contains("Editing"),
            "In-progress edit should show 'Editing' header, got: {}",
            lines[0]
        );
        assert!(
            lines[0].contains("src/main.rs"),
            "In-progress edit header should include path, got: {}",
            lines[0]
        );
    }

    #[test]
    fn completed_edit_fallthrough_has_semantic_header() {
        // Completed edit that falls through to generic rendering (no file_changes)
        let snapshot = make_edit_snapshot(ToolPhase::Completed, "config.toml", vec![], None);
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/test-cwd"), false);
        let lines = render_lines(&cell.display_lines(80));

        assert!(
            lines[0].contains("Edited"),
            "Completed edit fallthrough should show 'Edited' header, got: {}",
            lines[0]
        );
        assert!(
            lines[0].contains("config.toml"),
            "Completed edit header should include path, got: {}",
            lines[0]
        );
    }

    #[test]
    fn failed_edit_with_error_in_raw_output_shows_error_text() {
        let snapshot = make_edit_snapshot(
            ToolPhase::Failed,
            "README.md",
            vec![],
            Some(serde_json::json!({"error": "Permission denied: README.md"})),
        );
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/test-cwd"), false);
        let lines = render_lines(&cell.display_lines(80));

        assert!(
            lines.iter().any(|l| l.contains("Permission denied")),
            "Failed edit with raw_output error should show error text, got: {lines:?}"
        );
    }

    #[test]
    fn failed_edit_with_no_artifacts_shows_failed_fallback() {
        let snapshot = make_edit_snapshot(ToolPhase::Failed, "README.md", vec![], None);
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/test-cwd"), false);
        let lines = render_lines(&cell.display_lines(80));

        assert!(
            lines.iter().any(|l| l.contains("(failed)")),
            "Failed edit with no artifacts should show '(failed)' fallback, got: {lines:?}"
        );
    }

    #[test]
    fn failed_edit_with_diff_artifact_renders_diff() {
        let snapshot = make_edit_snapshot(
            ToolPhase::Failed,
            "README.md",
            vec![nori_protocol::Artifact::Diff(nori_protocol::FileChange {
                path: PathBuf::from("README.md"),
                old_text: Some("# Old\n".into()),
                new_text: "# New\n".into(),
            })],
            None,
        );
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/test-cwd"), false);
        let lines = render_lines(&cell.display_lines(80));

        // Should have red bullet (failed)
        let raw_lines = cell.display_lines(80);
        let bullet_span = &raw_lines[0].spans[0];
        assert!(
            bullet_span.style.fg == Some(ratatui::style::Color::Red),
            "Failed edit with diff should still have red bullet"
        );

        // Should render diff content — look for the changed file path
        // and the actual content from old/new text
        assert!(
            lines.iter().any(|l| l.contains("README.md")),
            "Failed edit with diff artifact should render file path, got: {lines:?}"
        );
    }

    #[test]
    fn completed_non_edit_tool_still_has_dim_bullet() {
        let snapshot = ToolSnapshot {
            call_id: "call-fetch-1".into(),
            title: "Fetch resource".into(),
            kind: ToolKind::Fetch,
            phase: ToolPhase::Completed,
            locations: vec![],
            invocation: None,
            artifacts: vec![Artifact::Text {
                text: "200 OK".into(),
            }],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        };
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/test-cwd"), false);
        let lines = cell.display_lines(80);

        let bullet_span = &lines[0].spans[0];
        // Should NOT be red (it's completed, not failed)
        assert!(
            bullet_span.style.fg != Some(ratatui::style::Color::Red),
            "Completed non-edit tool should NOT have red bullet, got {:?}",
            bullet_span.style
        );
    }

    #[test]
    fn failed_move_has_semantic_header() {
        let snapshot = ToolSnapshot {
            call_id: "call-move-1".into(),
            title: "Move old.rs to new.rs".into(),
            kind: ToolKind::Move,
            phase: ToolPhase::Failed,
            locations: vec![nori_protocol::ToolLocation {
                path: PathBuf::from("old.rs"),
                line: None,
            }],
            invocation: None,
            artifacts: vec![],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        };
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/test-cwd"), false);
        let lines = render_lines(&cell.display_lines(80));

        assert!(
            lines[0].contains("Move failed:"),
            "Failed move should show 'Move failed:' header, got: {}",
            lines[0]
        );
    }

    // --- Spec 11: Delete File Operation Bridge ---

    #[test]
    fn completed_delete_has_green_bullet_and_semantic_header() {
        let snapshot = ToolSnapshot {
            call_id: "call-del-1".into(),
            title: "Delete temp.txt".into(),
            kind: ToolKind::Delete,
            phase: ToolPhase::Completed,
            locations: vec![nori_protocol::ToolLocation {
                path: PathBuf::from("temp.txt"),
                line: None,
            }],
            invocation: Some(nori_protocol::Invocation::FileOperations {
                operations: vec![nori_protocol::FileOperation::Delete {
                    path: PathBuf::from("temp.txt"),
                    old_text: Some("old content\n".into()),
                }],
            }),
            artifacts: vec![],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        };
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/test-cwd"), false);
        let raw_lines = cell.display_lines(80);
        let lines = render_lines(&raw_lines);

        // Green bullet for completed delete
        let bullet_span = &raw_lines[0].spans[0];
        assert!(
            bullet_span.style.fg == Some(ratatui::style::Color::Green),
            "Completed delete should have green bullet, got {:?}",
            bullet_span.style
        );

        // Semantic header
        assert!(
            lines[0].contains("Deleted"),
            "Completed delete should show 'Deleted' header, got: {}",
            lines[0]
        );
        assert!(
            lines[0].contains("temp.txt"),
            "Completed delete header should include path, got: {}",
            lines[0]
        );
    }

    #[test]
    fn completed_move_has_green_bullet_and_semantic_header() {
        let snapshot = ToolSnapshot {
            call_id: "call-move-2".into(),
            title: "Move old.rs to new.rs".into(),
            kind: ToolKind::Move,
            phase: ToolPhase::Completed,
            locations: vec![nori_protocol::ToolLocation {
                path: PathBuf::from("old.rs"),
                line: None,
            }],
            invocation: Some(nori_protocol::Invocation::FileOperations {
                operations: vec![nori_protocol::FileOperation::Move {
                    from_path: PathBuf::from("old.rs"),
                    to_path: PathBuf::from("new.rs"),
                    old_text: Some("content\n".into()),
                    new_text: Some("content\n".into()),
                }],
            }),
            artifacts: vec![],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        };
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/test-cwd"), false);
        let raw_lines = cell.display_lines(80);
        let lines = render_lines(&raw_lines);

        // Green bullet for completed move
        let bullet_span = &raw_lines[0].spans[0];
        assert!(
            bullet_span.style.fg == Some(ratatui::style::Color::Green),
            "Completed move should have green bullet, got {:?}",
            bullet_span.style
        );

        // Semantic header
        assert!(
            lines[0].contains("Moved"),
            "Completed move should show 'Moved' header, got: {}",
            lines[0]
        );
    }

    #[test]
    fn completed_edit_with_file_operations_invocation_renders_diff() {
        // When a completed edit has FileOperations invocation (no Artifact::Diff),
        // it should still render diff content from the invocation data.
        let snapshot = ToolSnapshot {
            call_id: "call-edit-ops".into(),
            title: "Edit src/lib.rs".into(),
            kind: ToolKind::Edit,
            phase: ToolPhase::Completed,
            locations: vec![nori_protocol::ToolLocation {
                path: PathBuf::from("src/lib.rs"),
                line: None,
            }],
            invocation: Some(nori_protocol::Invocation::FileOperations {
                operations: vec![nori_protocol::FileOperation::Update {
                    path: PathBuf::from("src/lib.rs"),
                    old_text: "fn old() {}\n".into(),
                    new_text: "fn new() {}\n".into(),
                }],
            }),
            artifacts: vec![],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        };
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/test-cwd"), false);
        let lines = render_lines(&cell.display_lines(80));

        // Should show "Edited" header
        assert!(
            lines[0].contains("Edited"),
            "Completed edit with FileOperations should show 'Edited', got: {}",
            lines[0]
        );

        // Should render diff content from FileOperations
        let has_diff = lines
            .iter()
            .any(|l| l.contains("lib.rs") || l.contains('+') || l.contains('-'));
        assert!(
            has_diff,
            "Completed edit with FileOperations should render diff content, got: {lines:?}"
        );
    }

    #[test]
    fn file_operations_update_diff_uses_file_line_numbers() {
        let dir = tempfile::tempdir().expect("temp dir");
        write_updated_rust_fixture(dir.path(), "src/lib.rs", 50, "fn line_50_updated() {}");

        let changes = changes_from_invocation(
            &Some(nori_protocol::Invocation::FileOperations {
                operations: vec![nori_protocol::FileOperation::Update {
                    path: PathBuf::from("src/lib.rs"),
                    old_text: "fn line_50() {}\n".into(),
                    new_text: "fn line_50_updated() {}\n".into(),
                }],
            }),
            dir.path(),
        );

        let change = changes
            .get(&PathBuf::from("src/lib.rs"))
            .expect("change should exist");
        let codex_core::protocol::FileChange::Update { unified_diff, .. } = change else {
            panic!("expected update diff, got {change:?}");
        };
        assert!(
            unified_diff.contains("@@ -50 +50 @@") || unified_diff.contains("@@ -50,1 +50,1 @@"),
            "expected contextual hunk header for line 50, got:\n{unified_diff}"
        );
    }

    #[test]
    fn file_operations_move_diff_uses_file_line_numbers() {
        let dir = tempfile::tempdir().expect("temp dir");
        write_updated_rust_fixture(dir.path(), "src/old.rs", 75, "fn renamed_line_75() {}");

        let changes = changes_from_invocation(
            &Some(nori_protocol::Invocation::FileOperations {
                operations: vec![nori_protocol::FileOperation::Move {
                    from_path: PathBuf::from("src/old.rs"),
                    to_path: PathBuf::from("src/new.rs"),
                    old_text: Some("fn line_75() {}\n".into()),
                    new_text: Some("fn renamed_line_75() {}\n".into()),
                }],
            }),
            dir.path(),
        );

        let change = changes
            .get(&PathBuf::from("src/old.rs"))
            .expect("change should exist");
        let codex_core::protocol::FileChange::Update { unified_diff, .. } = change else {
            panic!("expected update diff, got {change:?}");
        };
        assert!(
            unified_diff.contains("@@ -75 +75 @@") || unified_diff.contains("@@ -75,1 +75,1 @@"),
            "expected contextual hunk header for line 75, got:\n{unified_diff}"
        );
    }

    #[test]
    fn edit_cell_keeps_contextual_diff_when_file_disappears_before_render() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file_path =
            write_updated_rust_fixture(dir.path(), "src/lib.rs", 50, "fn line_50_updated() {}");

        let snapshot = ToolSnapshot {
            call_id: "call-edit-ops".into(),
            title: "Edit src/lib.rs".into(),
            kind: ToolKind::Edit,
            phase: ToolPhase::Completed,
            locations: vec![nori_protocol::ToolLocation {
                path: PathBuf::from("src/lib.rs"),
                line: None,
            }],
            invocation: Some(nori_protocol::Invocation::FileOperations {
                operations: vec![nori_protocol::FileOperation::Update {
                    path: PathBuf::from("src/lib.rs"),
                    old_text: "fn line_50() {}\n".into(),
                    new_text: "fn line_50_updated() {}\n".into(),
                }],
            }),
            artifacts: vec![],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        };
        let cell = ClientToolCell::new(snapshot, dir.path().to_path_buf(), false);
        std::fs::remove_file(&file_path).expect("remove file before render");

        let lines = render_lines(&cell.display_lines(80));

        assert!(
            lines.iter().any(|line| line.contains("50 -fn line_50")),
            "expected cached contextual line number in rendered diff, got: {lines:?}"
        );
    }

    // --- Spec 13: Deduplicate single-file edit header ---

    #[test]
    fn single_file_edit_shows_verb_path_counts_once() {
        let snapshot = ToolSnapshot {
            call_id: "call-dedup".into(),
            title: "Edit README.md".into(),
            kind: ToolKind::Edit,
            phase: ToolPhase::Completed,
            locations: vec![nori_protocol::ToolLocation {
                path: PathBuf::from("README.md"),
                line: None,
            }],
            invocation: None,
            artifacts: vec![nori_protocol::Artifact::Diff(nori_protocol::FileChange {
                path: PathBuf::from("README.md"),
                old_text: Some("# Old Title\n".into()),
                new_text: "# New Title\n".into(),
            })],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        };
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/test-cwd"), false);
        let lines = render_lines(&cell.display_lines(80));

        // Count how many lines contain "Edited README.md" — should be exactly 1
        let edit_header_count = lines
            .iter()
            .filter(|l| l.contains("Edited") && l.contains("README.md"))
            .count();
        assert_eq!(
            edit_header_count, 1,
            "Single-file edit should show 'Edited README.md' exactly once, got {edit_header_count} in: {lines:?}"
        );
        // The single header should include line counts
        let header_with_counts = lines
            .iter()
            .find(|l| l.contains("Edited") && l.contains("README.md"))
            .expect("should have an edit header");
        assert!(
            header_with_counts.contains("+1") && header_with_counts.contains("-1"),
            "Single-file edit header should include line counts, got: {header_with_counts}"
        );
    }

    #[test]
    fn multi_file_edit_uses_aggregate_header() {
        let snapshot = ToolSnapshot {
            call_id: "call-multi-edit".into(),
            title: "Edit multiple files".into(),
            kind: ToolKind::Edit,
            phase: ToolPhase::Completed,
            locations: vec![
                nori_protocol::ToolLocation {
                    path: PathBuf::from("README.md"),
                    line: None,
                },
                nori_protocol::ToolLocation {
                    path: PathBuf::from("src/lib.rs"),
                    line: None,
                },
            ],
            invocation: None,
            artifacts: vec![
                nori_protocol::Artifact::Diff(nori_protocol::FileChange {
                    path: PathBuf::from("README.md"),
                    old_text: Some("# Old\n".into()),
                    new_text: "# New\n".into(),
                }),
                nori_protocol::Artifact::Diff(nori_protocol::FileChange {
                    path: PathBuf::from("src/lib.rs"),
                    old_text: Some("fn old() {}\n".into()),
                    new_text: "fn new() {}\n".into(),
                }),
            ],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        };
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/test-cwd"), false);
        let lines = render_lines(&cell.display_lines(80));

        // First line should be the aggregate header with file count
        assert!(
            lines[0].contains("Edited") && lines[0].contains("2 files"),
            "Multi-file edit header should show 'Edited 2 files' as the first line, got: {:?}",
            lines[0]
        );
        // No subsequent line should repeat the aggregate header
        let has_duplicate_aggregate = lines[1..]
            .iter()
            .any(|l| l.contains("Edited") && l.contains("files"));
        assert!(
            !has_duplicate_aggregate,
            "Should not have a duplicate aggregate header in: {lines:?}"
        );
    }

    // --- Edit cell indentation should match PatchHistoryCell (4 spaces, not 8) ---

    #[test]
    fn edit_cell_diff_lines_have_single_indent() {
        let snapshot = ToolSnapshot {
            call_id: "call-edit-indent".into(),
            title: "Write README.md".into(),
            kind: ToolKind::Edit,
            phase: ToolPhase::Completed,
            locations: vec![nori_protocol::ToolLocation {
                path: PathBuf::from("README.md"),
                line: None,
            }],
            invocation: Some(Invocation::FileChanges {
                changes: vec![nori_protocol::FileChange {
                    path: PathBuf::from("README.md"),
                    old_text: None,
                    new_text: "hello\nworld\n".into(),
                }],
            }),
            artifacts: vec![nori_protocol::Artifact::Diff(nori_protocol::FileChange {
                path: PathBuf::from("README.md"),
                old_text: None,
                new_text: "hello\nworld\n".into(),
            })],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        };
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/"), false);
        let lines = render_lines(&cell.display_lines(80));

        // The diff content lines (containing "+hello", "+world") should start
        // with exactly 4 spaces, not 8. This matches PatchHistoryCell's output.
        let diff_content: Vec<_> = lines
            .iter()
            .filter(|l| l.contains("+hello") || l.contains("+world"))
            .collect();
        assert!(
            !diff_content.is_empty(),
            "expected diff content lines, got: {lines:?}"
        );
        for line in &diff_content {
            assert!(
                line.starts_with("    ") && !line.starts_with("        "),
                "diff content should have 4-space indent (not 8): {line:?}"
            );
        }
    }

    #[test]
    fn move_cell_diff_lines_have_single_indent() {
        let snapshot = ToolSnapshot {
            call_id: "call-move".into(),
            title: "Move old.rs".into(),
            kind: ToolKind::Move,
            phase: ToolPhase::Completed,
            locations: vec![nori_protocol::ToolLocation {
                path: PathBuf::from("old.rs"),
                line: None,
            }],
            invocation: Some(Invocation::FileOperations {
                operations: vec![nori_protocol::FileOperation::Move {
                    from_path: PathBuf::from("old.rs"),
                    to_path: PathBuf::from("new.rs"),
                    old_text: None,
                    new_text: Some("content\n".into()),
                }],
            }),
            artifacts: vec![],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        };
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/"), false);
        let lines = render_lines(&cell.display_lines(80));

        // Header should say "Moved", not "Edited"
        assert!(
            lines[0].contains("Moved"),
            "move header should say 'Moved', got: {}",
            lines[0]
        );

        // Diff content lines should have 4-space indent, not 8
        let diff_content: Vec<_> = lines.iter().filter(|l| l.contains("+content")).collect();
        assert!(
            !diff_content.is_empty(),
            "expected diff content lines for move, got: {lines:?}"
        );
        for line in &diff_content {
            assert!(
                line.starts_with("    ") && !line.starts_with("        "),
                "move diff content should have 4-space indent (not 8): {line:?}"
            );
        }
    }

    // --- Bug fix: Read/Search should auto-detect as exploring ---

    #[test]
    fn standalone_read_renders_as_explored_not_generic() {
        // A Read tool that is NOT explicitly mark_exploring() should still render
        // using the exploring format ("Explored") when it's a completed Read,
        // not the generic "Tool [completed]" format.
        let snapshot = ToolSnapshot {
            call_id: "call-read-auto".into(),
            title: "Read README.md".into(),
            kind: ToolKind::Read,
            phase: ToolPhase::Completed,
            locations: vec![],
            invocation: Some(Invocation::Read {
                path: "README.md".into(),
            }),
            artifacts: vec![],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        };
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/cwd"), false);
        let lines = render_lines(&cell.display_lines(80));

        assert!(
            lines[0].contains("Explored"),
            "Standalone completed Read should auto-render as 'Explored', got: {}",
            lines[0]
        );
        assert!(
            !lines[0].contains("Tool ["),
            "Standalone Read should NOT use generic format, got: {}",
            lines[0]
        );
    }

    #[test]
    fn standalone_search_renders_as_explored_not_generic() {
        let snapshot = ToolSnapshot {
            call_id: "call-search-auto".into(),
            title: "Search TODO".into(),
            kind: ToolKind::Search,
            phase: ToolPhase::Completed,
            locations: vec![],
            invocation: Some(Invocation::Search {
                query: Some("TODO".into()),
                path: Some("/repo/src".into()),
            }),
            artifacts: vec![],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        };
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/cwd"), false);
        let lines = render_lines(&cell.display_lines(80));

        assert!(
            lines[0].contains("Explored"),
            "Standalone completed Search should auto-render as 'Explored', got: {}",
            lines[0]
        );
    }

    // --- Bug fix: Execute transcript should use $ command format ---

    #[test]
    fn execute_transcript_uses_shell_format() {
        // In transcript view, Execute tools should render as "$ command"
        // (shell-style), not "• Ran command" (bullet-style).
        let snapshot = make_execute_snapshot(
            ToolPhase::Completed,
            "date --utc",
            vec![Artifact::Text {
                text: "2026-03-30".into(),
            }],
            Some(serde_json::json!({"exit_code": 0, "stdout": "2026-03-30"})),
        );
        let cell = ClientToolCell::new(snapshot, PathBuf::from("/tmp/cwd"), false);
        let lines = render_lines(&cell.transcript_lines(80));

        // Should have "$ " prefix (shell-style)
        assert!(
            lines[0].contains("$ "),
            "Execute transcript should use '$ ' shell format, got: {}",
            lines[0]
        );
        assert!(
            lines[0].contains("date --utc"),
            "Execute transcript should show command, got: {}",
            lines[0]
        );
        // Should NOT have bullet-style "Ran"
        assert!(
            !lines.iter().any(|l| l.contains("Ran")),
            "Execute transcript should NOT use bullet-style 'Ran', got: {lines:?}"
        );
    }

    // --- Bug fix: Exploring transcript should use exploring format ---

    #[test]
    fn exploring_transcript_uses_explored_format_not_shell() {
        // In transcript view, Read/Search tools should render as "Explored"
        // with sub-items, NOT as "$ Read /path" (shell-style).
        let snap = ToolSnapshot {
            call_id: "call-read-t".into(),
            title: "Read file.rs".into(),
            kind: ToolKind::Read,
            phase: ToolPhase::Completed,
            locations: vec![],
            invocation: Some(Invocation::Read {
                path: "file.rs".into(),
            }),
            artifacts: vec![],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        };
        let mut cell = ClientToolCell::new(snap, PathBuf::from("/tmp/cwd"), false);
        cell.mark_exploring();
        let lines = render_lines(&cell.transcript_lines(80));

        assert!(
            lines[0].contains("Explored"),
            "Exploring transcript should show 'Explored', got: {}",
            lines[0]
        );
        assert!(
            !lines.iter().any(|l| l.starts_with("$ ")),
            "Exploring transcript should NOT use '$ ' shell format, got: {lines:?}"
        );
    }
}

use std::time::Instant;

use ratatui::prelude::*;
use ratatui::style::Stylize;
use textwrap::WordSplitter;

use crate::client_event_format::format_artifacts;
use crate::client_event_format::format_invocation;
use crate::client_event_format::format_tool_header;
use crate::client_event_format::is_exploring_snapshot;
use crate::client_event_format::is_invocation_redundant;
use crate::client_event_format::strip_code_fences;
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

#[derive(Debug)]
pub(crate) struct ClientToolCell {
    snapshot: nori_protocol::ToolSnapshot,
    animations_enabled: bool,
    start_time: Option<Instant>,
}

impl ClientToolCell {
    pub(crate) fn new(snapshot: nori_protocol::ToolSnapshot, animations_enabled: bool) -> Self {
        let start_time = if is_active_phase(&snapshot.phase) {
            Some(Instant::now())
        } else {
            None
        };
        Self {
            snapshot,
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

    pub(crate) fn is_exploring(&self) -> bool {
        is_exploring_snapshot(&self.snapshot)
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
        self.snapshot = snapshot;
    }

    pub(crate) fn mark_failed(&mut self) {
        if self.is_active() {
            self.snapshot.phase = nori_protocol::ToolPhase::Failed;
            self.start_time = None;
        }
    }

    fn render_generic_lines(&self) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let bullet = if self.is_active() {
            spinner(self.start_time, self.animations_enabled)
        } else {
            "•".dim()
        };
        lines.push(Line::from(vec![
            bullet,
            " ".into(),
            format_tool_header(&self.snapshot).bold(),
        ]));

        let mut details = Vec::new();
        if let Some(invocation) = format_invocation(&self.snapshot.invocation)
            && !is_invocation_redundant(&invocation, &self.snapshot.title)
        {
            details.push(invocation);
        }
        for artifact in format_artifacts(&self.snapshot.artifacts) {
            if artifact.contains('\n') {
                details.extend(artifact.lines().map(str::to_string));
            } else {
                details.push(artifact);
            }
        }

        // Fallback: show location paths when no other detail lines were produced
        if details.is_empty() {
            for location in &self.snapshot.locations {
                details.push(location.path.display().to_string());
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

    // Fall back to artifact text, stripping code fences
    for artifact in &snapshot.artifacts {
        if let nori_protocol::Artifact::Text { text } = artifact
            && !text.is_empty()
        {
            return Some(strip_code_fences(text));
        }
    }

    None
}

impl HistoryCell for ClientToolCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        if self.snapshot.kind == nori_protocol::ToolKind::Execute {
            self.render_execute_lines(width)
        } else {
            self.render_generic_lines()
        }
    }

    fn transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
        if self.snapshot.kind == nori_protocol::ToolKind::Execute {
            self.render_execute_lines(width)
        } else {
            self.render_generic_lines()
        }
    }
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
        let cell = ClientToolCell::new(snapshot, false);
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
        let cell = ClientToolCell::new(snapshot, false);
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
        let cell = ClientToolCell::new(snapshot, false);
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
        let cell = ClientToolCell::new(snapshot, false);
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
        let cell = ClientToolCell::new(snapshot, false);
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
    fn non_execute_tool_uses_generic_rendering() {
        let snapshot = ToolSnapshot {
            call_id: "call-2".into(),
            title: "Read /repo/README.md".into(),
            kind: ToolKind::Read,
            phase: ToolPhase::Completed,
            locations: vec![],
            invocation: Some(Invocation::Read {
                path: "/repo/README.md".into(),
            }),
            artifacts: vec![],
            raw_input: None,
            raw_output: None,
        };
        let cell = ClientToolCell::new(snapshot, false);
        let lines = render_lines(&cell.display_lines(80));

        // Should use the generic format
        assert!(
            lines[0].contains("Tool [completed]"),
            "Non-execute tool should use generic format, got: {}",
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
        let cell = ClientToolCell::new(snapshot, false);
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
        let cell = ClientToolCell::new(snapshot, false);
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
        };
        let cell = ClientToolCell::new(snapshot, false);
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
        };
        let cell = ClientToolCell::new(snapshot, false);
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
            title: "README.md".into(),
            kind: ToolKind::Read,
            phase: ToolPhase::Completed,
            locations: vec![],
            invocation: None,
            artifacts: vec![],
            raw_input: None,
            raw_output: None,
        };
        let cell = ClientToolCell::new(snapshot, false);
        let lines = render_lines(&cell.display_lines(80));

        // Should still render at least a header line
        assert!(!lines.is_empty(), "Expected at least a header line");
        assert!(
            lines[0].contains("README.md"),
            "Header should contain the title, got: {}",
            lines[0]
        );
    }

    // --- Spec 06: Artifact Text Output Cleanup ---

    #[test]
    fn generic_tool_strips_code_fences_from_text_artifact() {
        let snapshot = ToolSnapshot {
            call_id: "call-fence".into(),
            title: "Read /repo/src/main.rs".into(),
            kind: ToolKind::Read,
            phase: ToolPhase::Completed,
            locations: vec![],
            invocation: Some(Invocation::Read {
                path: "/repo/src/main.rs".into(),
            }),
            artifacts: vec![Artifact::Text {
                text: "```rust\nfn main() {}\n```".into(),
            }],
            raw_input: None,
            raw_output: None,
        };
        let cell = ClientToolCell::new(snapshot, false);
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
        };
        let cell = ClientToolCell::new(snapshot, false);
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
            title: "Search 'pattern'".into(),
            kind: ToolKind::Search,
            phase: ToolPhase::Completed,
            locations: vec![],
            invocation: Some(Invocation::Search {
                query: Some("pattern".into()),
                path: None,
            }),
            artifacts: vec![Artifact::Text {
                text: "line one\nline two\nline three".into(),
            }],
            raw_input: None,
            raw_output: None,
        };
        let cell = ClientToolCell::new(snapshot, false);
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
            title: "Read /repo/README.md".into(),
            kind: ToolKind::Read,
            phase: ToolPhase::Completed,
            locations: vec![],
            invocation: Some(Invocation::Read {
                path: "/repo/README.md".into(),
            }),
            artifacts: vec![Artifact::Text {
                text: "# README".into(),
            }],
            raw_input: None,
            raw_output: None,
        };
        let cell = ClientToolCell::new(snapshot, false);
        let lines = render_lines(&cell.display_lines(80));

        // The invocation detail line ("Read: /repo/README.md") should NOT appear
        // because the title already contains the same information
        assert!(
            !lines.iter().any(|l| l.contains("Read: /repo/README.md")),
            "Redundant invocation should be omitted when title contains same info, got: {lines:?}"
        );
        // But the artifact output should still appear
        assert!(
            lines.iter().any(|l| l.contains("# README")),
            "Output content should still render, got: {lines:?}"
        );
    }
}

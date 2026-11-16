use std::ops::Range;
use std::time::Instant;

use nori_harness::bash::summarize;
use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::style::Stylize;

use crate::bottom_pane::truncate_line;
use crate::render::highlight::highlight_bash_to_lines;
use crate::shimmer::shimmer_spans;

/// Leading segments that only set up the environment for the real work.
const SETUP_COMMANDS: &[&str] = &["cd", "export", "pushd"];

/// Commands that only read files or list the tree.
const READ_COMMANDS: &[&str] = &["cat", "head", "tail", "less", "nl", "bat", "sed"];
const SEARCH_COMMANDS: &[&str] = &[
    "grep", "egrep", "fgrep", "rg", "ag", "ack", "find", "fd", "ls", "tree", "wc", "sort", "uniq",
    "cut", "tr", "column", "stat", "file", "du", "pwd", "which",
];

pub(crate) fn spinner(start_time: Option<Instant>, animations_enabled: bool) -> Span<'static> {
    if !animations_enabled {
        return "•".dim();
    }
    let elapsed = start_time.map(|st| st.elapsed()).unwrap_or_default();
    if supports_color::on_cached(supports_color::Stream::Stdout)
        .map(|level| level.has_16m)
        .unwrap_or(false)
    {
        shimmer_spans("•")[0].clone()
    } else {
        let blink_on = (elapsed.as_millis() / 600).is_multiple_of(2);
        if blink_on { "•".into() } else { "◦".dim() }
    }
}

fn basename(text: &str) -> &str {
    text.rsplit('/').next().unwrap_or(text)
}

/// One-line summary of `command` that fits in `max_width` cells: command
/// names bold, arguments as a faint `…`, segments that do not fit as `+N more`.
pub(crate) fn compact_command(command: &str, max_width: usize) -> Vec<Span<'static>> {
    let Some(segments) = summarize(command) else {
        let first = command.lines().next().unwrap_or_default();
        return truncate_line(&Line::from(first.to_string()), max_width).spans;
    };
    let setup = segments
        .iter()
        .take_while(|seg| {
            let [stage] = seg.stages.as_slice() else {
                return false;
            };
            SETUP_COMMANDS.contains(&basename(&command[stage.name.clone()]))
        })
        .count()
        .min(segments.len() - 1);
    let shown = &segments[setup..];

    let mut spans: Vec<Span<'static>> = Vec::new();
    for (index, segment) in shown.iter().enumerate() {
        let mut candidate: Vec<Span<'static>> = Vec::new();
        if index > 0 {
            candidate.push(format!(" {} ", segment.separator.unwrap_or(";")).dim());
        }
        for (stage_index, stage) in segment.stages.iter().enumerate() {
            if stage_index > 0 {
                candidate.push(" | ".dim());
            }
            candidate.push(basename(&command[stage.name.clone()]).to_string().bold());
            if let Some(script) = &stage.script {
                candidate.extend([
                    " ".into(),
                    basename(&command[script.clone()]).to_string().bold(),
                ]);
            }
            if stage.has_args {
                candidate.extend([" ".into(), "…".dim()]);
            }
            if let Some(lines) = stage.heredoc_lines {
                candidate.push(format!(" <<heredoc ({lines} lines)").dim());
            }
        }
        let rest = shown.len() - index - 1;
        let more = (rest > 0).then(|| format!(" +{rest} more").len());
        let used: usize = spans.iter().chain(&candidate).map(Span::width).sum();
        if index > 0 && used + more.unwrap_or(0) > max_width {
            spans.push(format!(" +{} more", shown.len() - index).dim());
            break;
        }
        spans.extend(candidate);
    }
    truncate_line(&Line::from(spans), max_width).spans
}

/// The whole command, syntax highlighted, with command names in bold.
pub(crate) fn full_command_lines(command: &str) -> Vec<Line<'static>> {
    let names: Vec<Range<usize>> = summarize(command)
        .into_iter()
        .flatten()
        .flat_map(|segment| segment.stages)
        .flat_map(|stage| [Some(stage.name), stage.script])
        .flatten()
        .collect();
    let mut line_start = 0;
    let mut lines = highlight_bash_to_lines(command);
    for (line, source) in lines.iter_mut().zip(command.split('\n')) {
        let mut offset = line_start;
        line_start += source.len() + 1;
        let mut spans = Vec::new();
        for span in line.spans.drain(..) {
            let start = offset;
            let end = start + span.content.len();
            offset = end;
            let mut cuts: Vec<usize> = names
                .iter()
                .flat_map(|range| [range.start, range.end])
                .filter(|cut| *cut > start && *cut < end)
                .collect();
            cuts.sort_unstable();
            cuts.push(end);
            let mut cursor = start;
            for cut in cuts {
                let Some(piece) = span.content.get(cursor - start..cut - start) else {
                    continue;
                };
                let bold = names.iter().any(|r| r.start <= cursor && cut <= r.end);
                let modifier = if bold {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                };
                spans.push(Span::styled(
                    piece.to_string(),
                    span.style.add_modifier(modifier),
                ));
                cursor = cut;
            }
        }
        line.spans = spans;
    }
    lines
}

/// The last non-blank output line, dim, red when the command failed.
pub(crate) fn last_output_line(
    output: &str,
    failed: bool,
    max_width: usize,
) -> Option<Line<'static>> {
    let last = output.lines().rev().find(|line| !line.trim().is_empty())?;
    let plain = codex_ansi_escape::ansi_escape_line(last)
        .to_string()
        .trim()
        .to_string();
    let span = if failed { plain.red() } else { plain.dim() };
    Some(truncate_line(&Line::from(span), max_width))
}

/// Whether every stage of `command` only reads or searches files.
pub(crate) fn is_read_only(command: &str) -> bool {
    let Some(segments) = summarize(command) else {
        return false;
    };
    segments.iter().flat_map(|seg| &seg.stages).all(|stage| {
        let name = basename(&command[stage.name.clone()]);
        let args: Vec<&str> = command[stage.text.clone()]
            .split_whitespace()
            .skip(1)
            .collect();
        let has = |flags: &[&str]| {
            args.iter()
                .any(|arg| flags.iter().any(|f| arg.starts_with(f)))
        };
        let operands = || args.iter().filter(|arg| !arg.starts_with('-'));
        let short = |flag: char| {
            args.iter()
                .any(|arg| arg.starts_with('-') && !arg.starts_with("--") && arg.contains(flag))
        };
        let safe = match name {
            "sed" => {
                !short('i')
                    && !has(&["--in-place"])
                    && operands().next().is_some_and(|script| {
                        let script = script.trim_matches(['\'', '"']);
                        let prints_lines = short('n')
                            && script
                                .chars()
                                .all(|c| c.is_ascii_digit() || ",$p;".contains(c));
                        let substitutes = script.strip_prefix('s').is_some_and(|rest| {
                            let delimiter = rest.chars().next().unwrap_or('/');
                            let parts: Vec<&str> = rest.split(delimiter).collect();
                            parts.len() == 4
                                && parts[3].chars().all(|c| "gIip0123456789".contains(c))
                        });
                        prints_lines || substitutes
                    })
            }
            "find" => !has(&["-delete", "-exec", "-ok", "-fprint", "-fls"]),
            "sort" | "tree" => !has(&["-o", "--output"]),
            "uniq" => operands().count() <= 1,
            "fd" => !has(&["-x", "-X", "--exec"]),
            "rg" => !has(&["--pre"]),
            _ => true,
        };
        let known = SETUP_COMMANDS.contains(&name)
            || READ_COMMANDS.contains(&name)
            || SEARCH_COMMANDS.contains(&name);
        known && safe && !stage.opaque && !stage.writes_file && stage.heredoc_lines.is_none()
    })
}

/// Whether a read-only command reads files rather than searching for them.
pub(crate) fn reads_files(command: &str) -> bool {
    summarize(command)
        .into_iter()
        .flatten()
        .flat_map(|segment| segment.stages)
        .map(|stage| basename(&command[stage.name]).to_string())
        .find(|name| !SETUP_COMMANDS.contains(&name.as_str()))
        .is_some_and(|name| READ_COMMANDS.contains(&name.as_str()))
}

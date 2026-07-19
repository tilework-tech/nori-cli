use std::time::Instant;

use super::CommandOutput;
use crate::shimmer::shimmer_spans;
use codex_ansi_escape::ansi_escape_line;
use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::style::Stylize;

pub(crate) const TOOL_CALL_MAX_LINES: usize = 5;

pub(crate) struct OutputLinesParams {
    pub(crate) line_limit: usize,
    pub(crate) only_err: bool,
    pub(crate) include_angle_pipe: bool,
    pub(crate) include_prefix: bool,
}

#[derive(Clone)]
pub(crate) struct OutputLines {
    pub(crate) lines: Vec<Line<'static>>,
    pub(crate) omitted: Option<usize>,
}

pub(crate) fn output_lines(
    output: Option<&CommandOutput>,
    params: OutputLinesParams,
) -> OutputLines {
    let OutputLinesParams {
        line_limit,
        only_err,
        include_angle_pipe,
        include_prefix,
    } = params;
    let CommandOutput {
        aggregated_output, ..
    } = match output {
        Some(output) if only_err && output.exit_code == 0 => {
            return OutputLines {
                lines: Vec::new(),
                omitted: None,
            };
        }
        Some(output) => output,
        None => {
            return OutputLines {
                lines: Vec::new(),
                omitted: None,
            };
        }
    };

    let src = aggregated_output;
    let lines: Vec<&str> = src.lines().collect();
    let total = lines.len();
    let mut out: Vec<Line<'static>> = Vec::new();

    let head_end = total.min(line_limit);
    for (i, raw) in lines[..head_end].iter().enumerate() {
        let mut line = ansi_escape_line(raw);
        let prefix = if !include_prefix {
            ""
        } else if i == 0 && include_angle_pipe {
            "  └ "
        } else {
            "    "
        };
        line.spans.insert(0, prefix.into());
        line.spans.iter_mut().for_each(|span| {
            span.style = span.style.add_modifier(Modifier::DIM);
        });
        out.push(line);
    }

    let show_ellipsis = total > 2 * line_limit;
    let omitted = if show_ellipsis {
        Some(total - 2 * line_limit)
    } else {
        None
    };
    if show_ellipsis {
        let omitted = total - 2 * line_limit;
        out.push(format!("… +{omitted} lines").into());
    }

    let tail_start = if show_ellipsis {
        total - line_limit
    } else {
        head_end
    };
    for raw in lines[tail_start..].iter() {
        let mut line = ansi_escape_line(raw);
        if include_prefix {
            line.spans.insert(0, "    ".into());
        }
        line.spans.iter_mut().for_each(|span| {
            span.style = span.style.add_modifier(Modifier::DIM);
        });
        out.push(line);
    }

    OutputLines {
        lines: out,
        omitted,
    }
}

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

pub(crate) fn limit_lines_from_start(lines: &[Line<'static>], keep: usize) -> Vec<Line<'static>> {
    if lines.len() <= keep {
        return lines.to_vec();
    }
    if keep == 0 {
        return vec![ellipsis_line(lines.len())];
    }

    let mut out: Vec<Line<'static>> = lines[..keep].to_vec();
    out.push(ellipsis_line(lines.len() - keep));
    out
}

pub(crate) fn truncate_lines_middle(
    lines: &[Line<'static>],
    max: usize,
    omitted_hint: Option<usize>,
) -> Vec<Line<'static>> {
    if max == 0 {
        return Vec::new();
    }
    if lines.len() <= max {
        return lines.to_vec();
    }
    if max == 1 {
        // Carry forward any previously omitted count and add any
        // additionally hidden content lines from this truncation.
        let base = omitted_hint.unwrap_or(0);
        // When an existing ellipsis is present, `lines` already includes
        // that single representation line; exclude it from the count of
        // additionally omitted content lines.
        let extra = lines
            .len()
            .saturating_sub(usize::from(omitted_hint.is_some()));
        let omitted = base + extra;
        return vec![ellipsis_line(omitted)];
    }

    let head = (max - 1) / 2;
    let tail = max - head - 1;
    let mut out: Vec<Line<'static>> = Vec::new();

    if head > 0 {
        out.extend(lines[..head].iter().cloned());
    }

    let base = omitted_hint.unwrap_or(0);
    let additional = lines
        .len()
        .saturating_sub(head + tail)
        .saturating_sub(usize::from(omitted_hint.is_some()));
    out.push(ellipsis_line(base + additional));

    if tail > 0 {
        out.extend(lines[lines.len() - tail..].iter().cloned());
    }

    out
}

pub(crate) fn ellipsis_line(omitted: usize) -> Line<'static> {
    Line::from(vec![format!("… +{omitted} lines").dim()])
}

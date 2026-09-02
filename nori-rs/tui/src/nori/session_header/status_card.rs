//! Pure views over [`StatusViewModel`].
//!
//! Both status renderings live here: the compact welcome block and the full
//! `/status` card. They read nothing but the model, so the storybook renders
//! exactly what a session renders.
//!
//! Layout rules (from the unbordered status design): no surface, no border,
//! plain labels without punctuation in an aligned column, and a two-cell
//! gutter before values. The provider name is the only coloured element on the
//! agent row; models, thought levels, separators, and agent-specific values
//! stay in the terminal foreground.

use crate::nori::token_count::format_token_count;
use crate::system_info::NoriVersionSource;
use crate::ui_types::format_si_suffix;
use nori_tui_components::Theme;
use ratatui::prelude::*;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::style::Stylize;
use unicode_width::UnicodeWidthStr;

use super::AgentKindSimple;
use super::InstructionFile;
use super::TokenCount;
use super::format_directory;
use super::status_view::AgentStatus;
use super::status_view::ContextStatus;
use super::status_view::GitStatus;
use super::status_view::StatusViewModel;

/// Width of the aligned label column shared by every labelled status row.
/// Sized to fit the widest label (`Forked from`) without punctuation. Values
/// begin after a separate two-cell gutter.
pub(super) const STATUS_LABEL_WIDTH: usize = 11;

/// Leading inset plus label and the minimum two-cell value separator.
pub(super) const STATUS_VALUE_OFFSET: usize = 2 + STATUS_LABEL_WIDTH + 2;

/// Maximum length for the task summary row.
const MAX_TASK_SUMMARY_LENGTH: usize = 50;

/// Build an aligned, inset label row without punctuation. A minimum two-cell
/// gutter separates the label and value, matching `DetailPane` columns.
pub(super) fn status_row(label: &str, value_spans: Vec<Span<'static>>) -> Line<'static> {
    let label = label.trim_end_matches(':');
    let padded = format!("  {label:<STATUS_LABEL_WIDTH$}  ");
    let mut spans = vec![Span::from(padded).dim()];
    spans.extend(value_spans);
    Line::from(spans)
}

/// Semantic identity styling for the provider name only. Supporting model,
/// thought level, and agent-specific values must remain in the terminal
/// foreground.
pub(super) fn agent_name_style(agent_kind: Option<AgentKindSimple>) -> Style {
    let theme = Theme::default();
    match agent_kind {
        #[allow(clippy::disallowed_methods)]
        Some(AgentKindSimple::Claude) => Style::new().fg(Color::Rgb(255, 158, 100)),
        Some(AgentKindSimple::Codex) => theme.provider_codex,
        Some(AgentKindSimple::Gemini) => theme.provider_gemini,
        None => theme.text,
    }
}

/// The compact agent row: the provider name, then each configured option's
/// current value in display order. Boolean toggles read by presence, so a
/// disabled toggle contributes nothing. Before the agent advertises any
/// configuration this is the provider name alone.
pub(super) fn agent_row_spans(agent: &AgentStatus) -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled(
        agent.provider.clone(),
        agent_name_style(agent.kind),
    )];
    for option in &agent.options {
        if let Some(text) = option.compact_text() {
            spans.push(Span::from(" · ").dim());
            spans.push(Span::from(text));
        }
    }
    spans
}

/// The full-card agent block: one row per advertised option, in the agent's own
/// aligned column, indented to the status value column.
fn agent_detail_lines(agent: &AgentStatus) -> Vec<Line<'static>> {
    let label_width = agent
        .options
        .iter()
        .map(|option| option.label.width())
        .max()
        .unwrap_or(0);

    agent
        .options
        .iter()
        .map(|option| {
            let label = &option.label;
            Line::from(vec![
                Span::from(" ".repeat(STATUS_VALUE_OFFSET)),
                Span::from(format!("{label:<label_width$}  ")).dim(),
                Span::from(option.display_value()),
            ])
        })
        .collect()
}

/// Render the active local instruction-file outline used by full `/status`.
/// Each file remains individually visible so engineers can audit context.
pub(super) fn instruction_file_lines(
    instruction_files: &[InstructionFile],
    inner_width: usize,
) -> Vec<Line<'static>> {
    // `Instructions` is wider than the shared label column, so the block gets
    // its own column and every row in it lines up under the first path.
    const LABEL: &str = "Instructions";
    let label_width = LABEL.width();
    let value_offset = 2 + label_width + 2;

    let active_files: Vec<&InstructionFile> = instruction_files
        .iter()
        .filter(|file| file.active)
        .collect();
    if active_files.is_empty() {
        return Vec::new();
    }

    let mut lines = Vec::new();
    let mut total_count: i64 = 0;
    let mut any_approximate = false;

    for (index, file) in active_files.iter().enumerate() {
        let value_width = inner_width.saturating_sub(value_offset);
        let label = if index == 0 { LABEL } else { "" };
        let mut value_spans = Vec::new();
        if let Some(token_count) = &file.token_count {
            total_count += token_count.count;
            any_approximate |= token_count.approximate;
            let count = format_token_count(token_count);
            let path_budget = value_width.saturating_sub(count.width() + 2);
            let path = format_directory(&file.path, Some(path_budget));
            let gap = value_width.saturating_sub(path.width() + count.width());
            value_spans.push(Span::from(path));
            value_spans.push(Span::from(" ".repeat(gap)));
            value_spans.push(Span::from(count).dim());
        } else {
            value_spans.push(Span::from(format_directory(&file.path, Some(value_width))));
        }
        lines.push(instruction_row(label, label_width, value_spans));
    }

    if total_count > 0 {
        let total = format_token_count(&TokenCount {
            count: total_count,
            approximate: any_approximate,
        });
        let file_count = active_files.len();
        let noun = if file_count == 1 { "file" } else { "files" };
        lines.push(instruction_row(
            "",
            label_width,
            vec![Span::from(format!("{file_count} {noun} · {total}")).dim()],
        ));
    }

    lines
}

/// A row in the instruction block: same inset and gutter as a status row, but
/// with the block's own wider label column.
fn instruction_row(
    label: &str,
    label_width: usize,
    value_spans: Vec<Span<'static>>,
) -> Line<'static> {
    let mut spans = vec![Span::from(format!("  {label:<label_width$}  ")).dim()];
    spans.extend(value_spans);
    Line::from(spans)
}

/// Build the value spans for the `Git` row, mirroring the footer's
/// GitBranch/WorktreeName/GitStats formatting but as a single row. Returns
/// `None` when no git branch is known.
pub(super) fn git_row_spans(git: &GitStatus) -> Option<Vec<Span<'static>>> {
    let branch = git.branch.as_ref()?;

    let mut spans: Vec<Span<'static>> = if git.is_worktree {
        vec![
            Span::from("⎇ ").light_red(),
            Span::from(branch.clone()).light_red(),
        ]
    } else {
        #[allow(clippy::disallowed_methods)]
        let yellow_branch = vec![
            Span::from("⎇ ").yellow(),
            Span::from(branch.clone()).yellow(),
        ];
        yellow_branch
    };

    if git.is_worktree
        && let Some(name) = &git.worktree_name
    {
        spans.push(Span::from(format!(" (worktree: {name})")).light_red());
    }

    if let (Some(added), Some(removed)) = (git.lines_added, git.lines_removed)
        && (added > 0 || removed > 0)
    {
        spans.push(Span::from(format!(" +{added}")).green());
        spans.push(Span::from(" ").dim());
        spans.push(Span::from(format!("-{removed}")).red());
    }

    if git.has_untracked {
        spans.push(Span::from(" ").dim());
        spans.push(Span::from("!").red().bold());
    }

    Some(spans)
}

/// Build the consolidated, codex-style context value, e.g.
/// `73% left (43.0K used / 272K)`. `percent_used` is the percentage *used*, so
/// the displayed "left" figure is its complement. Falls back to the leaner
/// forms when the used/window token counts are not available. Returns `None`
/// when there is no context percentage to show.
pub(super) fn context_value(context: &ContextStatus) -> Option<String> {
    let percent_used = context.percent_used?;
    let percent_left = (100 - percent_used).clamp(0, 100);
    Some(match (context.tokens, context.window_tokens) {
        (Some(used), Some(window)) => {
            let used_fmt = format_si_suffix(used);
            let window_fmt = format_si_suffix(window);
            format!("{percent_left}% left ({used_fmt} used / {window_fmt})")
        }
        (Some(used), None) => {
            let used_fmt = format_si_suffix(used);
            format!("{percent_left}% left ({used_fmt} used)")
        }
        (None, _) => format!("{percent_left}% left"),
    })
}

/// The compact welcome block: where the session runs, and which agent runs it.
pub(super) fn compact_lines(model: &StatusViewModel, inner_width: usize) -> Vec<Line<'static>> {
    let location = match &model.cloud_session {
        Some(cloud) => match &cloud.title {
            Some(title) => format!("{} ({title})", cloud.id),
            None => cloud.id.clone(),
        },
        None => format_directory(
            &model.directory,
            Some(inner_width.saturating_sub(STATUS_VALUE_OFFSET)),
        ),
    };

    let mut system_parts = vec![location];
    if let Some(approval_mode) = &model.approval_mode_label {
        system_parts.push(format!("{approval_mode} approvals"));
    }
    if let Some(skillset) = &model.skillset.name {
        system_parts.push(skillset.clone());
    }

    let mut system_spans = Vec::new();
    for (index, part) in system_parts.into_iter().enumerate() {
        if index > 0 {
            system_spans.push(Span::from(" · ").dim());
        }
        system_spans.push(Span::from(part));
    }

    vec![
        status_row("System", system_spans),
        status_row("Agent", agent_row_spans(&model.agent_status())),
    ]
}

/// The full `/status` card: session metadata, then the agent's configuration,
/// then the local instruction files that shape its context.
pub(super) fn full_lines(model: &StatusViewModel, inner_width: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    let dir_max_width = inner_width.saturating_sub(STATUS_VALUE_OFFSET);
    lines.push(status_row(
        "Directory",
        vec![Span::from(format_directory(
            &model.directory,
            Some(dir_max_width),
        ))],
    ));

    // Session row: the local conversation id (or the cloud id when the
    // conversation id is not yet known), with the broker title in parens on a
    // cloud session.
    let session_base = match (model.conversation_id, &model.cloud_session) {
        (Some(id), _) => Some(id.to_string()),
        (None, Some(cloud)) => Some(cloud.id.clone()),
        (None, None) => None,
    };
    if let Some(base) = session_base {
        let session_display = match model.cloud_session.as_ref().and_then(|c| c.title.as_ref()) {
            Some(title) => format!("{base} ({title})"),
            None => base,
        };
        lines.push(status_row("Session ID", vec![Span::from(session_display)]));
    }

    // The parent conversation after a branch-at-head fork stays resumable via
    // `nori resume <id>`.
    if let Some(forked_from) = model.forked_from {
        lines.push(status_row(
            "Forked from",
            vec![Span::from(forked_from.to_string())],
        ));
    }

    if let Some(title) = &model.session_title {
        lines.push(status_row("Title", vec![Span::from(title.clone())]));
    }

    if let Some(summary) = &model.prompt_summary {
        lines.push(status_row(
            "Summary",
            vec![Span::from(truncate_summary(summary, MAX_TASK_SUMMARY_LENGTH)).dim()],
        ));
    }

    let skillset_display = match &model.skillset.name {
        Some(name) => match &model.skillset.version {
            Some(version) => {
                let label = model
                    .skillset
                    .version_source
                    .map(NoriVersionSource::label)
                    .unwrap_or("Skillsets");
                format!("{name} ({label} v{version})")
            }
            None => name.clone(),
        },
        None => "(none)".to_string(),
    };
    lines.push(status_row("Skillset", vec![Span::from(skillset_display)]));

    if let Some(approval_mode) = &model.approval_mode_label {
        lines.push(status_row(
            "Approvals",
            vec![Span::from(approval_mode.clone())],
        ));
    }

    if let Some(git_spans) = git_row_spans(&model.git) {
        lines.push(status_row("Git", git_spans));
    }

    if let Some(context) = context_value(&model.context) {
        lines.push(status_row("Context", vec![Span::from(context)]));
    }

    if let Some(token_breakdown) = &model.token_breakdown {
        let total = token_breakdown.total();
        if total > 0 {
            let total_fmt = format_si_suffix(total);
            let mut token_spans = vec![Span::from(format!("{total_fmt} total")).dim()];
            if token_breakdown.cached_tokens > 0 {
                let cached_fmt = format_si_suffix(token_breakdown.cached_tokens);
                token_spans.push(Span::from(format!(" ({cached_fmt} cached)")).dim());
            }
            lines.push(status_row("Tokens", token_spans));
        }
    }

    // The agent and its configuration are their own block: everything the
    // agent decides, in one place, instead of scattered through the session
    // metadata above.
    let agent = model.agent_status();
    lines.push(Line::from(""));
    lines.push(status_row(
        "Agent",
        vec![Span::styled(
            agent.provider.clone(),
            agent_name_style(agent.kind),
        )],
    ));
    lines.extend(agent_detail_lines(&agent));

    // Local `/status` keeps the individual active instruction-file outline.
    // Cloud sessions suppress it because local discovery does not describe the
    // remote agent's actual context.
    if model.cloud_session.is_none() {
        let instruction_lines = instruction_file_lines(&model.instruction_files, inner_width);
        if !instruction_lines.is_empty() {
            lines.push(Line::from(""));
            lines.extend(instruction_lines);
        }
    }

    lines
}

/// Truncate a summary string to fit on one line.
pub(super) fn truncate_summary(summary: &str, max_len: usize) -> String {
    if summary.chars().count() <= max_len {
        summary.to_string()
    } else {
        let truncated_chars = max_len.saturating_sub(3);
        let truncated: String = summary.chars().take(truncated_chars).collect();
        format!("{truncated}...")
    }
}

#[cfg(test)]
mod tests;

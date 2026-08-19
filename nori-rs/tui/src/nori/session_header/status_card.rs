//! Data and row helpers for the enhanced `/status` card.
//!
//! The `/status` card is a by-default superset of the footer's information
//! categories. [`StatusCardInfo`] carries the footer-derived values the card
//! needs (git, ACP mode, skillset version, and the rich context breakdown) so
//! they can be threaded from the bottom pane into the header cell in one shot.

use crate::system_info::NoriVersionSource;
use crate::ui_types::format_si_suffix;
use ratatui::prelude::*;
use ratatui::style::Stylize;

/// Width of the aligned label column shared by every labelled card row.
/// Sized to fit the widest label (`forked from:`).
pub(super) const STATUS_LABEL_WIDTH: usize = 13;

/// Footer-derived values surfaced on the `/status` card so it stays a superset
/// of the footer's information categories regardless of the user's footer
/// configuration. Populated from `ChatComposer::footer_props()`.
#[derive(Debug, Clone, Default)]
pub(crate) struct StatusCardInfo {
    /// Current git branch, when the cwd is inside a git repo.
    pub(crate) git_branch: Option<String>,
    /// Whether the cwd is a git worktree (not the main checkout).
    pub(crate) is_worktree: bool,
    /// The worktree directory name, when in a worktree.
    pub(crate) worktree_name: Option<String>,
    /// Added lines relative to the branch's merge base.
    pub(crate) git_lines_added: Option<i32>,
    /// Removed lines relative to the branch's merge base.
    pub(crate) git_lines_removed: Option<i32>,
    /// Whether there are untracked, non-ignored files.
    pub(crate) git_has_untracked: bool,
    /// ACP agent mode label (e.g. "Plan", "Build") when the agent exposes modes.
    pub(crate) acp_mode_label: Option<String>,
    /// Agent-supplied session title from ACP session-info updates.
    pub(crate) session_title: Option<String>,
    /// Detected Nori skillsets version.
    pub(crate) nori_version: Option<String>,
    /// The source of the version detection (affects the display label).
    pub(crate) nori_version_source: Option<NoriVersionSource>,
    /// Tokens currently used in the context window.
    pub(crate) context_tokens: Option<i64>,
    /// Maximum tokens available in the context window.
    pub(crate) context_window_tokens: Option<i64>,
    /// Context window percentage used (0-100).
    pub(crate) context_window_percent: Option<i64>,
}

/// Build an aligned label row: a dim, left-padded label followed by the value
/// spans. Keeps the growing label column consistent across every card row.
pub(super) fn status_row(label: &str, value_spans: Vec<Span<'static>>) -> Line<'static> {
    let padded = format!("{label:<STATUS_LABEL_WIDTH$}");
    let mut spans = vec![Span::from(padded).dim()];
    spans.extend(value_spans);
    Line::from(spans)
}

/// Build the value spans for the `git:` row, mirroring the footer's
/// GitBranch/WorktreeName/GitStats formatting but as a single row. Returns
/// `None` when no git branch is known.
pub(super) fn git_row_spans(info: &StatusCardInfo) -> Option<Vec<Span<'static>>> {
    let branch = info.git_branch.as_ref()?;

    let mut spans: Vec<Span<'static>> = if info.is_worktree {
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

    if info.is_worktree
        && let Some(name) = &info.worktree_name
    {
        spans.push(Span::from(format!(" (worktree: {name})")).light_red());
    }

    if let (Some(added), Some(removed)) = (info.git_lines_added, info.git_lines_removed)
        && (added > 0 || removed > 0)
    {
        spans.push(Span::from(format!(" +{added}")).green());
        spans.push(Span::from(" ").dim());
        spans.push(Span::from(format!("-{removed}")).red());
    }

    if info.git_has_untracked {
        spans.push(Span::from(" ").dim());
        spans.push(Span::from("!").red().bold());
    }

    Some(spans)
}

/// Build the consolidated, codex-style context value, e.g.
/// `73% left (43.0K used / 272K)`. `context_window_percent` is the percentage
/// *used*, so the displayed "left" figure is its complement. Falls back to the
/// leaner forms when the used/window token counts are not available. Returns
/// `None` when there is no context percentage to show.
pub(super) fn context_value(info: &StatusCardInfo) -> Option<String> {
    let percent_used = info.context_window_percent?;
    let percent_left = (100 - percent_used).clamp(0, 100);
    Some(match (info.context_tokens, info.context_window_tokens) {
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

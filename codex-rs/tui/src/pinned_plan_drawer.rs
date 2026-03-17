//! Pinned plan drawer: renders the latest plan state as a fixed panel in the
//! viewport between the active cell and the bottom pane.
//!
//! Two modes are supported:
//! - **Expanded**: full checklist (via `PinnedPlanDrawer`)
//! - **Collapsed**: single-line progress summary (via `PinnedPlanDrawerCollapsed`)

use codex_protocol::plan_tool::StepStatus;
use codex_protocol::plan_tool::UpdatePlanArgs;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use unicode_width::UnicodeWidthStr;

use crate::history_cell::render_plan_lines;
use crate::render::renderable::Renderable;

/// A viewport-pinned widget that displays the latest plan as a checklist.
pub(crate) struct PinnedPlanDrawer<'a> {
    plan: &'a UpdatePlanArgs,
}

impl<'a> PinnedPlanDrawer<'a> {
    pub(crate) fn new(plan: &'a UpdatePlanArgs) -> Self {
        Self { plan }
    }
}

impl Renderable for PinnedPlanDrawer<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let lines = render_plan_lines(
            self.plan.explanation.as_deref(),
            &self.plan.plan,
            area.width,
        );
        Paragraph::new(Text::from(lines)).render(area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        let lines = render_plan_lines(self.plan.explanation.as_deref(), &self.plan.plan, width);
        u16::try_from(lines.len()).unwrap_or(u16::MAX)
    }
}

/// A viewport-pinned widget that displays a one-line plan progress summary.
pub(crate) struct PinnedPlanDrawerCollapsed<'a> {
    plan: &'a UpdatePlanArgs,
}

impl<'a> PinnedPlanDrawerCollapsed<'a> {
    pub(crate) fn new(plan: &'a UpdatePlanArgs) -> Self {
        Self { plan }
    }
}

impl Renderable for PinnedPlanDrawerCollapsed<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let line = build_collapsed_line(&self.plan.plan, area.width as usize);
        Paragraph::new(Text::from(line)).render(area, buf);
    }

    fn desired_height(&self, _width: u16) -> u16 {
        1
    }
}

/// Build the collapsed summary line for the plan.
///
/// Format: `Plan: X/Y completed  *  > Current: step_name`
fn build_collapsed_line(
    plan: &[codex_protocol::plan_tool::PlanItemArg],
    width: usize,
) -> Line<'static> {
    let total = plan.len();
    let completed = plan
        .iter()
        .filter(|s| matches!(s.status, StepStatus::Completed))
        .count();

    let progress = format!("{completed}/{total} completed");
    let progress_width = progress.width();
    let mut spans: Vec<Span<'static>> = vec!["Plan: ".bold(), progress.dim()];

    // Find the current (in-progress) step, or fall back to next pending, or "All done".
    let current_step = plan
        .iter()
        .find(|s| matches!(s.status, StepStatus::InProgress));
    let next_pending = plan
        .iter()
        .find(|s| matches!(s.status, StepStatus::Pending));

    let (label, step_name, use_cyan) = if let Some(step) = current_step {
        ("Current: ", step.step.clone(), true)
    } else if let Some(step) = next_pending {
        ("Next: ", step.step.clone(), false)
    } else {
        ("", "All done".to_string(), false)
    };

    // Separator + step info
    let sep = "  \u{2022}  ";
    let prefix_marker = "\u{25b8} ";

    // Calculate how much width remains for the step name.
    let fixed_prefix_width =
        "Plan: ".width() + progress_width + sep.width() + prefix_marker.width() + label.width();

    if width > fixed_prefix_width + 3 {
        spans.push(sep.dim());
        spans.push(prefix_marker.into());
        if !label.is_empty() {
            spans.push(label.into());
        }

        let budget = width.saturating_sub(fixed_prefix_width);
        let truncated = truncate_to_width(&step_name, budget);

        if use_cyan {
            spans.push(truncated.cyan().bold());
        } else if completed == total && total > 0 {
            spans.push(truncated.green());
        } else {
            spans.push(truncated.dim());
        }
    }

    Line::from(spans)
}

/// Truncate a string to fit within `max_width` display columns, appending "..."
/// if truncation is needed.
fn truncate_to_width(s: &str, max_width: usize) -> Span<'static> {
    if s.width() <= max_width {
        return Span::from(s.to_string());
    }
    let ellipsis = "...";
    let target = max_width.saturating_sub(ellipsis.width());
    let mut end = 0;
    let mut w = 0;
    for ch in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if w + cw > target {
            break;
        }
        w += cw;
        end += ch.len_utf8();
    }
    Span::from(format!("{}{ellipsis}", &s[..end]))
}

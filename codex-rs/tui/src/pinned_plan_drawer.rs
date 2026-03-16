//! Pinned plan drawer: renders the latest plan state as a fixed panel in the
//! viewport between the active cell and the bottom pane.
//!
//! TODO: add a collapsible one-line summary mode (via settings or slash command).

use codex_protocol::plan_tool::UpdatePlanArgs;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Text;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

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

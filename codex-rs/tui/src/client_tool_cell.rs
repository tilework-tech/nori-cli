use std::time::Instant;

use ratatui::style::Stylize;
use ratatui::text::Line;

use crate::client_event_format::format_artifacts;
use crate::client_event_format::format_invocation;
use crate::client_event_format::format_tool_header;
use crate::client_event_format::is_exploring_snapshot;
use crate::exec_cell::spinner;
use crate::history_cell::HistoryCell;

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

    pub(crate) fn pending_call_ids(&self) -> Vec<String> {
        if self.is_active() {
            vec![self.snapshot.call_id.clone()]
        } else {
            Vec::new()
        }
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

    fn render_lines(&self) -> Vec<Line<'static>> {
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
        if let Some(invocation) = format_invocation(&self.snapshot.invocation) {
            details.push(invocation);
        }
        for artifact in format_artifacts(&self.snapshot.artifacts) {
            if artifact.contains('\n') {
                details.extend(artifact.lines().map(str::to_string));
            } else {
                details.push(artifact);
            }
        }

        for (idx, detail) in details.into_iter().enumerate() {
            let prefix = if idx == 0 { "  └ " } else { "    " };
            lines.push(Line::from(vec![prefix.dim(), detail.dim()]));
        }

        lines
    }
}

impl HistoryCell for ClientToolCell {
    fn display_lines(&self, _width: u16) -> Vec<Line<'static>> {
        self.render_lines()
    }

    fn transcript_lines(&self, _width: u16) -> Vec<Line<'static>> {
        self.render_lines()
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

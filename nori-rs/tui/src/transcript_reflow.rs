use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use ratatui::text::Line;

use crate::history_cell::AgentMarkdownCell;
use crate::history_cell::AgentMessageCell;
use crate::history_cell::HistoryCell;

pub(crate) const MAX_REFLOW_CELLS: usize = 1_000;
pub(crate) const MAX_REFLOW_ROWS: usize = 10_000;
pub(crate) const REFLOW_DEBOUNCE: Duration = Duration::from_millis(75);
const HISTORY_TRUNCATED_NOTICE: &str = "… history truncated";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WidthChange {
    Unchanged,
    Initialized,
    Scheduled,
    Cancelled,
}

#[derive(Debug, Default)]
pub(crate) struct TranscriptReflowState {
    last_observed_width: Option<u16>,
    last_reflow_width: Option<u16>,
    pending_until: Option<Instant>,
    resize_requested_during_stream: bool,
}

impl TranscriptReflowState {
    pub(crate) fn note_width(&mut self, width: u16, now: Instant) -> WidthChange {
        let Some(previous_width) = self.last_observed_width.replace(width) else {
            self.last_reflow_width = Some(width);
            return WidthChange::Initialized;
        };
        if previous_width == width {
            return WidthChange::Unchanged;
        }
        if self.last_reflow_width == Some(width) {
            self.cancel();
            return WidthChange::Cancelled;
        }
        self.pending_until = Some(now + REFLOW_DEBOUNCE);
        WidthChange::Scheduled
    }

    pub(crate) fn schedule_immediate(&mut self) {
        self.pending_until = Some(Instant::now());
    }

    pub(crate) fn cancel(&mut self) {
        self.pending_until = None;
        self.resize_requested_during_stream = false;
    }

    pub(crate) fn has_pending_reflow(&self) -> bool {
        self.pending_until.is_some()
    }

    pub(crate) fn pending_until(&self) -> Option<Instant> {
        self.pending_until
    }

    pub(crate) fn pending_is_due(&self, now: Instant) -> bool {
        self.pending_until.is_some_and(|deadline| now >= deadline)
    }

    #[cfg(test)]
    pub(crate) fn last_reflow_width(&self) -> Option<u16> {
        self.last_reflow_width
    }

    pub(crate) fn mark_reflowed(&mut self, width: u16) {
        self.last_reflow_width = Some(width);
        self.pending_until = None;
    }

    pub(crate) fn mark_resize_requested_during_stream(&mut self) {
        self.resize_requested_during_stream = true;
    }

    pub(crate) fn take_stream_finish_reflow_needed(&mut self) -> bool {
        std::mem::take(&mut self.resize_requested_during_stream)
    }
}

pub(crate) fn consolidate_agent_message_cells(
    transcript_cells: &mut Vec<Arc<dyn HistoryCell>>,
    source: String,
    cwd: &Path,
) -> Option<(std::ops::Range<usize>, Arc<dyn HistoryCell>)> {
    let end = transcript_cells.len();
    let mut start = end;
    while start > 0 {
        let cell = &transcript_cells[start - 1];
        if !cell.as_any().is::<AgentMessageCell>() {
            break;
        }
        start -= 1;
        if !cell.is_stream_continuation() {
            break;
        }
    }
    if start == end {
        return None;
    }

    let range = start..end;
    let replacement = Arc::new(AgentMarkdownCell::new(source, cwd)) as Arc<dyn HistoryCell>;
    transcript_cells.splice(range.clone(), std::iter::once(replacement.clone()));
    Some((range, replacement))
}

pub(crate) fn has_unconsolidated_agent_message(transcript_cells: &[Arc<dyn HistoryCell>]) -> bool {
    transcript_cells
        .last()
        .is_some_and(|cell| cell.as_any().is::<AgentMessageCell>())
}

pub(crate) fn render_transcript_tail(
    transcript_cells: &[Arc<dyn HistoryCell>],
    width: u16,
) -> Vec<Line<'static>> {
    render_transcript_tail_with_limits(transcript_cells, width, MAX_REFLOW_CELLS, MAX_REFLOW_ROWS)
}

fn render_transcript_tail_with_limits(
    transcript_cells: &[Arc<dyn HistoryCell>],
    width: u16,
    max_cells: usize,
    max_rows: usize,
) -> Vec<Line<'static>> {
    let start = transcript_cells.len().saturating_sub(max_cells);
    let mut truncated = start > 0;
    let mut rendered = Vec::new();
    let mut has_emitted = false;

    for cell in &transcript_cells[start..] {
        let mut lines = cell.display_lines(width);
        if lines.is_empty() {
            continue;
        }
        if !cell.is_stream_continuation() {
            if has_emitted {
                rendered.push(Line::default());
            } else {
                has_emitted = true;
            }
        }
        rendered.append(&mut lines);
    }
    let rendered = crate::insert_history::wrap_history_lines_for_width(&rendered, width);

    if physical_rows(&rendered, width) > max_rows {
        truncated = true;
    }
    if !truncated {
        return rendered;
    }

    let notice = Line::from(HISTORY_TRUNCATED_NOTICE);
    let notice_rows = physical_rows(std::slice::from_ref(&notice), width).saturating_add(1);
    let history_budget = max_rows.saturating_sub(notice_rows);
    let retained = retain_newest_rows(rendered, width, history_budget);
    if max_rows < notice_rows {
        return retained;
    }

    let mut result = Vec::with_capacity(retained.len() + 2);
    result.push(notice);
    result.push(Line::default());
    result.extend(retained);
    result
}

fn retain_newest_rows(
    lines: Vec<Line<'static>>,
    width: u16,
    max_rows: usize,
) -> Vec<Line<'static>> {
    let mut retained = VecDeque::new();
    let mut rows: usize = 0;
    for line in lines.into_iter().rev() {
        let line_rows = physical_rows(std::slice::from_ref(&line), width);
        if rows.saturating_add(line_rows) > max_rows {
            break;
        }
        rows += line_rows;
        retained.push_front(line);
    }
    retained.into_iter().collect()
}

fn physical_rows(lines: &[Line<'_>], width: u16) -> usize {
    let width = usize::from(width.max(1));
    lines
        .iter()
        .map(|line| line.width().max(1).div_ceil(width))
        .sum()
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;
    use std::time::Duration;
    use std::time::Instant;

    use pretty_assertions::assert_eq;
    use ratatui::text::Line;

    use super::*;
    use crate::history_cell::AgentMessageCell;
    use crate::history_cell::HistoryCell;
    use crate::history_cell::PlainHistoryCell;

    fn plain(lines: &[&str]) -> Arc<dyn HistoryCell> {
        Arc::new(PlainHistoryCell::new(
            lines
                .iter()
                .map(|line| Line::from((*line).to_string()))
                .collect(),
        ))
    }

    fn line_text(lines: &[Line<'_>]) -> Vec<String> {
        lines.iter().map(Line::to_string).collect()
    }

    #[test]
    fn first_width_observation_does_not_schedule_reflow() {
        let now = Instant::now();
        let mut state = TranscriptReflowState::default();

        assert_eq!(state.note_width(80, now), WidthChange::Initialized);
        assert!(!state.has_pending_reflow());
        assert_eq!(state.last_reflow_width(), Some(80));
    }

    #[test]
    fn width_changes_use_a_trailing_seventy_five_millisecond_debounce() {
        let started = Instant::now();
        let mut state = TranscriptReflowState::default();
        state.note_width(80, started);

        assert_eq!(state.note_width(100, started), WidthChange::Scheduled);
        assert!(!state.pending_is_due(started + Duration::from_millis(74)));

        let repeated = started + Duration::from_millis(50);
        assert_eq!(state.note_width(120, repeated), WidthChange::Scheduled);
        assert!(!state.pending_is_due(started + Duration::from_millis(124)));
        assert!(state.pending_is_due(started + Duration::from_millis(125)));
    }

    #[test]
    fn settling_at_the_last_reflowed_width_cancels_pending_work() {
        let now = Instant::now();
        let mut state = TranscriptReflowState::default();
        state.note_width(80, now);
        state.note_width(100, now);

        assert_eq!(state.note_width(80, now), WidthChange::Cancelled);
        assert!(!state.has_pending_reflow());
    }

    #[test]
    fn row_cap_can_trim_inside_one_large_cell_and_counts_the_notice() {
        let cells = vec![plain(&["one", "two", "three", "four", "five", "six"])];

        let rendered = render_transcript_tail_with_limits(&cells, 80, 10, 5);

        assert_eq!(
            line_text(&rendered),
            vec!["… history truncated", "", "four", "five", "six",]
        );
    }

    #[test]
    fn row_cap_counts_the_same_word_wrapping_as_history_insertion() {
        let cells = vec![plain(&["aaaaa bbbbb ccccc ddddd eeeee"])];

        let rendered = render_transcript_tail_with_limits(&cells, 10, 10, 4);

        assert_eq!(
            line_text(&rendered),
            vec!["… history truncated", "", "eeeee"]
        );
    }

    #[test]
    fn default_cell_cap_keeps_the_newest_thousand_without_mutating_transcript() {
        let cells = (0..1_001)
            .map(|index| plain(&[&format!("cell {index}")]))
            .collect::<Vec<_>>();

        let rendered = line_text(&render_transcript_tail(&cells, 80));

        assert_eq!(cells.len(), 1_001);
        assert_eq!(&rendered[..2], &["… history truncated", ""]);
        assert!(rendered.iter().any(|line| line == "cell 1"));
        assert!(!rendered.iter().any(|line| line == "cell 0"));
        assert_eq!(rendered.last().map(String::as_str), Some("cell 1000"));
    }

    #[test]
    fn default_row_cap_includes_the_notice_and_keeps_the_newest_rows() {
        let source = (0..10_001)
            .map(|index| format!("row {index}"))
            .collect::<Vec<_>>();
        let cell: Arc<dyn HistoryCell> = Arc::new(PlainHistoryCell::new(
            source.iter().cloned().map(Line::from).collect(),
        ));

        let rendered = line_text(&render_transcript_tail(&[cell], 80));

        assert_eq!(rendered.len(), 10_000);
        assert_eq!(&rendered[..2], &["… history truncated", ""]);
        assert_eq!(rendered[2], "row 3");
        assert_eq!(rendered.last().map(String::as_str), Some("row 10000"));
    }

    #[test]
    fn consolidation_preserves_neighbor_order_and_reflows_the_message_once() {
        let mut cells: Vec<Arc<dyn HistoryCell>> = vec![
            plain(&["tool"]),
            Arc::new(AgentMessageCell::new(vec![Line::from("first")], true)),
            Arc::new(AgentMessageCell::new(vec![Line::from("second")], false)),
        ];

        assert!(has_unconsolidated_agent_message(&cells));

        let consolidation = consolidate_agent_message_cells(
            &mut cells,
            "first paragraph with enough words to wrap at a narrow width\n\nsecond paragraph"
                .to_string(),
            Path::new("/tmp"),
        );

        assert!(consolidation.is_some());
        assert!(!has_unconsolidated_agent_message(&cells));
        assert_eq!(cells.len(), 2);
        let wide = line_text(&render_transcript_tail(&cells, 80));
        let narrow = line_text(&render_transcript_tail(&cells, 28));
        assert_eq!(wide.iter().filter(|line| line.contains("tool")).count(), 1);
        assert_eq!(wide.iter().filter(|line| line.contains("first")).count(), 1);
        assert_eq!(
            wide.iter().filter(|line| line.contains("second")).count(),
            1
        );
        assert_eq!(wide.first().map(String::as_str), Some("tool"));
        assert!(narrow.len() > wide.len());
    }
}

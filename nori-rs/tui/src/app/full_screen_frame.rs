//! One frame of the full-screen view: the active session's transcript above
//! the chat widget.
//!
//! The transcript is the same rows the inline view prints into the
//! scrollback. It is anchored to the bottom, so the newest rows sit just
//! above the chat widget, which keeps its normal height (including the turn
//! in progress, which lives in the widget).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::text::Text;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use super::transcript_cells::TranscriptCells;
use crate::insert_history::wrap_history_lines_for_width;
use crate::render::renderable::Renderable;
use crate::transcript_reflow::MAX_REFLOW_CELLS;

/// Draw the full-screen view into `area`. Returns where the cursor goes.
pub(crate) fn render_full_screen(
    area: Rect,
    buf: &mut Buffer,
    transcript: &TranscriptRows,
    bottom: &dyn Renderable,
) -> Option<(u16, u16)> {
    let bottom_height = bottom.desired_height(area.width).min(area.height);
    let transcript_height = area.height - bottom_height;
    let bottom_area = Rect::new(
        area.x,
        area.y + transcript_height,
        area.width,
        bottom_height,
    );
    let shown = transcript.tail(transcript_height);
    let shown_height = u16::try_from(shown.len()).unwrap_or(transcript_height);
    let transcript_area = Rect::new(
        area.x,
        area.y + transcript_height - shown_height,
        area.width,
        shown_height,
    );
    Paragraph::new(Text::from(shown)).render(transcript_area, buf);
    bottom.render(bottom_area, buf);
    bottom.cursor_pos(bottom_area)
}

/// What the cached rows were rendered for. Within one generation the
/// transcript only grows, so the cached rows of its earlier cells stay right.
#[derive(Clone, Copy, PartialEq, Eq)]
struct RenderedFor {
    generation: i64,
    width: u16,
}

/// The transcript rows for the full-screen view, wrapped to the screen width.
///
/// Each cell is rendered once: appending renders only the new cells, a tail
/// replacement (a finished reply becoming one Markdown cell) renders only the
/// replaced cells, and the whole tail is rendered again only when the width
/// changes or another edit moves the transcript's generation. Unlike the inline reprint, the view never
/// shows a truncation notice: it only shows the newest screenful.
#[derive(Default)]
pub(crate) struct TranscriptRows {
    rendered_for: Option<RenderedFor>,
    /// The tail replacements already in `cells`.
    tail_edits_seen: i64,
    /// The rows of each transcript cell, including the blank row that
    /// separates it from the cell before. Cells older than the rendered
    /// tail have no rows.
    cells: Vec<Vec<Line<'static>>>,
    has_emitted: bool,
}

impl TranscriptRows {
    pub(crate) fn update(&mut self, cells: &TranscriptCells, width: u16) {
        let rendered_for = RenderedFor {
            generation: cells.generation(),
            width,
        };
        if self.rendered_for != Some(rendered_for) {
            let skipped = cells.len().saturating_sub(MAX_REFLOW_CELLS);
            self.cells = vec![Vec::new(); skipped];
            self.has_emitted = false;
            self.rendered_for = Some(rendered_for);
        } else if let Some(changed) = cells.changed_since(self.tail_edits_seen) {
            self.cells.truncate(changed);
            self.has_emitted = self.cells.iter().any(|rows| !rows.is_empty());
        }
        self.tail_edits_seen = cells.tail_edits();
        for cell in &cells[self.cells.len()..] {
            let mut lines = cell.display_lines(width);
            if !lines.is_empty() && !cell.is_stream_continuation() {
                if self.has_emitted {
                    lines.insert(0, Line::default());
                } else {
                    self.has_emitted = true;
                }
            }
            self.cells.push(wrap_history_lines_for_width(&lines, width));
        }
    }

    /// The newest `height` rows, oldest first.
    pub(crate) fn tail(&self, height: u16) -> Vec<Line<'static>> {
        let mut tail: Vec<Line<'static>> = self
            .cells
            .iter()
            .rev()
            .flat_map(|rows| rows.iter().rev())
            .take(usize::from(height))
            .cloned()
            .collect();
        tail.reverse();
        tail
    }
}

#[cfg(test)]
mod tests {
    use super::TranscriptRows;
    use super::render_full_screen;
    use crate::app::transcript_cells::TranscriptCells;
    use crate::history_cell::AgentMessageCell;
    use crate::history_cell::HistoryCell;
    use crate::history_cell::new_user_prompt;
    use pretty_assertions::assert_eq;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::text::Line;
    use std::sync::Arc;
    use std::sync::atomic::AtomicI32;
    use std::sync::atomic::Ordering;

    fn frame(cells: &[Arc<dyn HistoryCell>], width: u16, height: u16) -> Terminal<TestBackend> {
        let (mut chat, _rx, _op_rx) = crate::chatwidget::tests::make_chatwidget_manual();
        chat.set_composer_text("unsent draft about the gulls".to_string());
        let cells = TranscriptCells::from(cells.to_vec());
        let mut transcript = TranscriptRows::default();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        terminal
            .draw(|frame| {
                let area = frame.area();
                transcript.update(&cells, area.width);
                render_full_screen(area, frame.buffer_mut(), &transcript, &chat);
            })
            .expect("draw");
        terminal
    }

    #[test]
    fn full_screen_frame_shows_the_transcript_above_the_composer() {
        let cells: Vec<Arc<dyn HistoryCell>> = vec![
            Arc::new(new_user_prompt("what did the keeper count".to_string())),
            Arc::new(AgentMessageCell::new(
                vec![Line::from("The lighthouse keeper counted nine gulls.")],
                true,
            )),
        ];
        insta::assert_snapshot!(frame(&cells, 60, 18).backend());
    }

    #[test]
    fn full_screen_frame_keeps_the_newest_rows_when_the_transcript_overflows() {
        let mut cells: Vec<Arc<dyn HistoryCell>> = Vec::new();
        for turn in 1..=12 {
            cells.push(Arc::new(new_user_prompt(format!("question {turn}"))));
            cells.push(Arc::new(AgentMessageCell::new(
                vec![Line::from(format!("answer {turn}"))],
                true,
            )));
        }
        insta::assert_snapshot!(frame(&cells, 60, 18).backend());
    }

    /// A cell that counts how often it is rendered.
    #[derive(Debug)]
    struct CountedCell {
        text: String,
        renders: Arc<AtomicI32>,
    }

    impl HistoryCell for CountedCell {
        fn display_lines(&self, _width: u16) -> Vec<Line<'static>> {
            self.renders.fetch_add(1, Ordering::Relaxed);
            vec![Line::from(self.text.clone())]
        }
    }

    fn counted(text: &str) -> (Arc<dyn HistoryCell>, Arc<AtomicI32>) {
        let renders = Arc::new(AtomicI32::new(0));
        let cell = CountedCell {
            text: text.to_string(),
            renders: renders.clone(),
        };
        (Arc::new(cell), renders)
    }

    fn texts(rows: &TranscriptRows) -> Vec<String> {
        rows.tail(u16::MAX)
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn appending_renders_only_the_new_cells() {
        let (first, first_renders) = counted("first");
        let (second, second_renders) = counted("second");
        let mut cells = TranscriptCells::from(vec![first, second]);
        let mut rows = TranscriptRows::default();
        rows.update(&cells, 40);
        for turn in 0..3 {
            cells.push(counted(&format!("appended {turn}")).0);
            rows.update(&cells, 40);
        }
        assert_eq!(
            [
                first_renders.load(Ordering::Relaxed),
                second_renders.load(Ordering::Relaxed)
            ],
            [1, 1]
        );
        assert_eq!(
            texts(&rows),
            [
                "first",
                "",
                "second",
                "",
                "appended 0",
                "",
                "appended 1",
                "",
                "appended 2"
            ]
        );
    }

    #[test]
    fn an_edit_that_keeps_the_length_and_the_last_cell_is_rendered() {
        let (last, _) = counted("last");
        let mut cells = TranscriptCells::from(vec![counted("before").0, last]);
        let mut rows = TranscriptRows::default();
        rows.update(&cells, 40);
        cells.edit()[0] = counted("after").0;
        rows.update(&cells, 40);
        assert_eq!(texts(&rows), ["after", "", "last"]);
    }

    #[test]
    fn replacing_the_tail_renders_only_the_replaced_cells() {
        let (first, first_renders) = counted("first");
        let (second, second_renders) = counted("second");
        let mut cells = TranscriptCells::from(vec![first, second, counted("partial").0]);
        let mut rows = TranscriptRows::default();
        rows.update(&cells, 40);

        cells.replace_tail(2..3, counted("whole").0);
        rows.update(&cells, 40);
        cells.push(counted("next").0);
        cells.replace_tail(3..4, counted("next whole").0);
        rows.update(&cells, 40);

        assert_eq!(
            [
                first_renders.load(Ordering::Relaxed),
                second_renders.load(Ordering::Relaxed)
            ],
            [1, 1]
        );
        assert_eq!(
            texts(&rows),
            ["first", "", "second", "", "whole", "", "next whole"]
        );
    }

    #[test]
    fn a_new_width_renders_every_cell_again() {
        let (cell, renders) = counted("only");
        let cells = TranscriptCells::from(vec![cell]);
        let mut rows = TranscriptRows::default();
        rows.update(&cells, 40);
        rows.update(&cells, 40);
        rows.update(&cells, 30);
        assert_eq!(renders.load(Ordering::Relaxed), 2);
    }
}

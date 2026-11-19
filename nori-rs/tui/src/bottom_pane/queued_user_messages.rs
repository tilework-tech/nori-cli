use crossterm::event::KeyCode;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use crate::key_hint;
use crate::render::renderable::Renderable;
use crate::wrapping::RtOptions;
use crate::wrapping::word_wrap_lines;

/// Widget that displays a list of user messages queued while a turn is in progress.
///
/// Rows mirror the harness queue. A row whose reclaim is in flight is hidden
/// at once, so the same message cannot be taken back twice; the next queue
/// snapshot confirms the removal.
pub(crate) struct QueuedUserMessages {
    rows: Vec<nori_protocol::QueueEntry>,
    reclaiming: Vec<String>,
}

impl QueuedUserMessages {
    pub(crate) fn new() -> Self {
        Self {
            rows: Vec::new(),
            reclaiming: Vec::new(),
        }
    }

    /// Replace the rows from a queue snapshot. Hosts that send only texts
    /// produce rows that cannot be reclaimed.
    pub(crate) fn set(&mut self, prompts: Vec<String>, entries: Vec<nori_protocol::QueueEntry>) {
        self.rows = if entries.is_empty() {
            prompts
                .into_iter()
                .map(|text| nori_protocol::QueueEntry {
                    id: String::new(),
                    text,
                    editable: false,
                })
                .collect()
        } else {
            entries
        };
        self.reclaiming
            .retain(|id| self.rows.iter().any(|row| &row.id == id));
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.visible_rows().next().is_none()
    }

    pub(crate) fn has_editable(&self) -> bool {
        self.visible_rows().any(|row| row.editable)
    }

    /// Hide and return the ID of the newest visible message that can return
    /// to the composer.
    pub(crate) fn start_reclaim_newest(&mut self) -> Option<String> {
        let id = self
            .visible_rows()
            .filter(|row| row.editable)
            .last()?
            .id
            .clone();
        self.reclaiming.push(id.clone());
        Some(id)
    }

    /// Forget an in-flight reclaim once the harness answered.
    pub(crate) fn finish_reclaim(&mut self, id: &str) {
        self.reclaiming.retain(|pending| pending != id);
    }

    fn visible_rows(&self) -> impl Iterator<Item = &nori_protocol::QueueEntry> {
        self.rows
            .iter()
            .filter(|row| !self.reclaiming.contains(&row.id))
    }

    fn as_renderable(&self, width: u16) -> Box<dyn Renderable> {
        if self.is_empty() || width < 4 {
            return Box::new(());
        }

        let mut lines = vec![];

        for row in self.visible_rows() {
            let wrapped = word_wrap_lines(
                row.text.lines().map(|line| line.dim().italic()),
                RtOptions::new(width as usize)
                    .initial_indent(Line::from("  ↳ ".dim()))
                    .subsequent_indent(Line::from("    ")),
            );
            let len = wrapped.len();
            for line in wrapped.into_iter().take(3) {
                lines.push(line);
            }
            if len > 3 {
                lines.push(Line::from("    …".dim().italic()));
            }
        }

        if self.has_editable() {
            lines.push(
                Line::from(vec![
                    "    ".into(),
                    key_hint::alt(KeyCode::Up).into(),
                    " edit".into(),
                ])
                .dim(),
            );
        }

        Paragraph::new(lines).into()
    }
}

impl Renderable for QueuedUserMessages {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }

        self.as_renderable(area.width).render(area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.as_renderable(width).desired_height(width)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;
    use pretty_assertions::assert_eq;

    fn queue_of(texts: &[&str]) -> QueuedUserMessages {
        let mut queue = QueuedUserMessages::new();
        queue.set(
            texts.iter().map(ToString::to_string).collect(),
            texts
                .iter()
                .enumerate()
                .map(|(index, text)| nori_protocol::QueueEntry {
                    id: format!("evt-{index}"),
                    text: text.to_string(),
                    editable: true,
                })
                .collect(),
        );
        queue
    }

    fn render(queue: &QueuedUserMessages, width: u16) -> Buffer {
        let height = queue.desired_height(width);
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        queue.render(Rect::new(0, 0, width, height), &mut buf);
        buf
    }

    #[test]
    fn desired_height_empty() {
        let queue = QueuedUserMessages::new();
        assert_eq!(queue.desired_height(40), 0);
    }

    #[test]
    fn desired_height_one_message() {
        let queue = queue_of(&["Hello, world!"]);
        assert_eq!(queue.desired_height(40), 2);
    }

    #[test]
    fn render_one_message() {
        let queue = queue_of(&["Hello, world!"]);
        assert_snapshot!("render_one_message", format!("{:?}", render(&queue, 40)));
    }

    #[test]
    fn render_two_messages() {
        let queue = queue_of(&["Hello, world!", "This is another message"]);
        assert_snapshot!("render_two_messages", format!("{:?}", render(&queue, 40)));
    }

    #[test]
    fn render_more_than_three_messages() {
        let queue = queue_of(&[
            "Hello, world!",
            "This is another message",
            "This is a third message",
            "This is a fourth message",
        ]);
        assert_snapshot!(
            "render_more_than_three_messages",
            format!("{:?}", render(&queue, 40))
        );
    }

    #[test]
    fn render_wrapped_message() {
        let queue = queue_of(&[
            "This is a longer message that should be wrapped",
            "This is another message",
        ]);
        assert_snapshot!(
            "render_wrapped_message",
            format!("{:?}", render(&queue, 40))
        );
    }

    #[test]
    fn render_many_line_message() {
        let queue = queue_of(&["This is\na message\nwith many\nlines"]);
        assert_snapshot!(
            "render_many_line_message",
            format!("{:?}", render(&queue, 40))
        );
    }

    #[test]
    fn rows_without_ids_render_without_the_edit_hint() {
        let mut queue = QueuedUserMessages::new();
        queue.set(vec!["From an older host".to_string()], Vec::new());
        assert_snapshot!(
            "rows_without_ids_render_without_the_edit_hint",
            format!("{:?}", render(&queue, 40))
        );
    }

    #[test]
    fn a_row_being_reclaimed_is_hidden_until_the_queue_confirms() {
        let mut queue = queue_of(&["first", "second"]);

        assert_eq!(queue.start_reclaim_newest().as_deref(), Some("evt-1"));
        assert_eq!(queue.start_reclaim_newest().as_deref(), Some("evt-0"));
        assert_eq!(queue.start_reclaim_newest(), None);
        assert!(queue.is_empty());
        assert_eq!(queue.desired_height(40), 0);
    }

    #[test]
    fn a_failed_reclaim_shows_the_row_again_while_it_is_still_queued() {
        let mut queue = queue_of(&["first"]);
        let id = queue.start_reclaim_newest().expect("editable row");

        queue.finish_reclaim(&id);

        assert_eq!(queue.desired_height(40), 2);
    }
}

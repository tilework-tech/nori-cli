use crossterm::event::KeyCode;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
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
///
/// The list can take focus. A focused list selects at most one editable row,
/// tracked by its stable ID so that duplicate texts never shift an edit or a
/// removal onto a different message. When the selected row leaves the queue
/// on its own (it started, for example), the list stays open with nothing
/// selected until the user picks again.
pub(crate) struct QueuedUserMessages {
    rows: Vec<nori_protocol::QueueEntry>,
    reclaiming: Vec<String>,
    paused: Option<nori_protocol::QueuePause>,
    focus: ListFocus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ListFocus {
    Blurred,
    Selected(String),
    /// The selected row left the queue. `index` is where it was among the
    /// editable rows, so the next j/k picks from the same place.
    Cleared {
        index: usize,
    },
}

impl QueuedUserMessages {
    pub(crate) fn new() -> Self {
        Self {
            rows: Vec::new(),
            reclaiming: Vec::new(),
            paused: None,
            focus: ListFocus::Blurred,
        }
    }

    /// Replace the rows from a queue snapshot. Hosts that send only texts
    /// produce rows that cannot be reclaimed. A selected row that left the
    /// queue clears the selection instead of moving it, so a following edit
    /// or removal cannot land on a message the user did not pick; with no
    /// editable row left the list loses focus.
    pub(crate) fn set(
        &mut self,
        prompts: Vec<String>,
        entries: Vec<nori_protocol::QueueEntry>,
        paused: Option<nori_protocol::QueuePause>,
    ) {
        let before_selected = self.editable_ids_before_selected();
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
        self.paused = paused;
        self.reclaiming
            .retain(|id| self.rows.iter().any(|row| &row.id == id));
        let ids = self.editable_ids();
        self.focus = match std::mem::replace(&mut self.focus, ListFocus::Blurred) {
            ListFocus::Blurred => ListFocus::Blurred,
            ListFocus::Selected(_) | ListFocus::Cleared { .. } if ids.is_empty() => {
                ListFocus::Blurred
            }
            ListFocus::Selected(id) if ids.contains(&id) => ListFocus::Selected(id),
            ListFocus::Selected(_) => ListFocus::Cleared {
                index: ids.iter().filter(|id| before_selected.contains(id)).count(),
            },
            ListFocus::Cleared { index } => ListFocus::Cleared { index },
        };
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.visible_rows().next().is_none()
    }

    pub(crate) fn has_editable(&self) -> bool {
        self.visible_rows().any(|row| row.editable)
    }

    pub(crate) fn is_paused(&self) -> bool {
        self.paused.is_some()
    }

    pub(crate) fn is_focused(&self) -> bool {
        self.focus != ListFocus::Blurred
    }

    /// Focus the list on the newest editable row. Returns `false` when there
    /// is nothing to select.
    pub(crate) fn focus(&mut self) -> bool {
        self.focus = match self.editable_ids().last() {
            Some(id) => ListFocus::Selected(id.clone()),
            None => ListFocus::Blurred,
        };
        self.is_focused()
    }

    pub(crate) fn blur(&mut self) {
        self.focus = ListFocus::Blurred;
    }

    /// Move the selection by `step` editable rows, stopping at either end.
    /// After the selection was cleared, j picks the row now in its place and
    /// k the one before it.
    pub(crate) fn move_selection(&mut self, step: isize) {
        let ids = self.editable_ids();
        let last = ids.len().saturating_sub(1);
        let target = match &self.focus {
            ListFocus::Blurred => return,
            ListFocus::Selected(_) => {
                let Some(index) = self.selected_index() else {
                    return;
                };
                index.saturating_add_signed(step)
            }
            ListFocus::Cleared { index } if step > 0 => *index,
            ListFocus::Cleared { index } => index.saturating_sub(1),
        };
        if let Some(id) = ids.get(target.min(last)) {
            self.focus = ListFocus::Selected(id.clone());
        }
    }

    /// Hide the selected row as an in-flight reclaim and return its ID.
    /// The list keeps focus on the neighbouring editable row, if any.
    /// Returns `None` when nothing is selected.
    pub(crate) fn start_reclaim_selected(&mut self) -> Option<String> {
        let index = self.selected_index()?;
        let ListFocus::Selected(id) = std::mem::replace(&mut self.focus, ListFocus::Blurred) else {
            return None;
        };
        self.reclaiming.push(id.clone());
        let ids = self.editable_ids();
        if let Some(next) = ids.get(index).or_else(|| ids.last()) {
            self.focus = ListFocus::Selected(next.clone());
        }
        Some(id)
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

    fn editable_ids(&self) -> Vec<String> {
        self.visible_rows()
            .filter(|row| row.editable)
            .map(|row| row.id.clone())
            .collect()
    }

    /// Position of the selection among editable rows, before any change.
    /// Editable visible rows above the selection, oldest first.
    fn editable_ids_before_selected(&self) -> Vec<String> {
        let ListFocus::Selected(selected) = &self.focus else {
            return Vec::new();
        };
        self.editable_ids()
            .into_iter()
            .take_while(|id| id != selected)
            .collect()
    }

    fn selected_index(&self) -> Option<usize> {
        let ListFocus::Selected(selected) = &self.focus else {
            return None;
        };
        let rows: Vec<&nori_protocol::QueueEntry> =
            self.rows.iter().filter(|row| row.editable).collect();
        let position = rows.iter().position(|row| &row.id == selected)?;
        Some(
            rows[..position]
                .iter()
                .filter(|row| !self.reclaiming.contains(&row.id))
                .count(),
        )
    }

    fn as_renderable(&self, width: u16) -> Box<dyn Renderable> {
        if self.is_empty() || width < 4 {
            return Box::new(());
        }

        let mut lines = vec![];

        if let Some(paused) = self.paused {
            let count = self.visible_rows().count();
            let cause = match paused {
                nori_protocol::QueuePause::Interrupted => "interrupted",
                nori_protocol::QueuePause::TurnFailed => "turn error",
            };
            lines.extend(word_wrap_lines(
                [Line::from(vec![
                    format!("{count} queued · ").dim(),
                    "paused".cyan(),
                    format!(" ({cause}) · ").dim(),
                    key_hint::plain(KeyCode::Enter).into(),
                    " resumes".dim(),
                ])],
                RtOptions::new(width as usize)
                    .initial_indent(Line::from("  "))
                    .subsequent_indent(Line::from("    ")),
            ));
        }

        for row in self.visible_rows() {
            let selected = matches!(&self.focus, ListFocus::Selected(id) if id == &row.id);
            let (initial, text_style): (Line, Style) = if selected {
                (
                    Line::from(vec!["  ".into(), "› ".green()]),
                    Style::default(),
                )
            } else {
                (
                    Line::from("  ↳ ".dim()),
                    Style::default().add_modifier(Modifier::DIM | Modifier::ITALIC),
                )
            };
            let wrapped = word_wrap_lines(
                row.text
                    .lines()
                    .map(|line| Line::from(Span::styled(line.to_string(), text_style))),
                RtOptions::new(width as usize)
                    .initial_indent(initial)
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

        if let ListFocus::Cleared { .. } = self.focus {
            lines.push(
                Line::from(vec![
                    "    selected message left the queue · j/k pick again · ".into(),
                    key_hint::plain(KeyCode::Esc).into(),
                    " back".into(),
                ])
                .dim(),
            );
        } else if self.is_focused() {
            lines.push(
                Line::from(vec![
                    "    j/k select · ".into(),
                    key_hint::plain(KeyCode::Enter).into(),
                    "/e edit · ".into(),
                    key_hint::plain(KeyCode::Delete).into(),
                    "/x remove · ".into(),
                    key_hint::plain(KeyCode::Esc).into(),
                    " back".into(),
                ])
                .dim(),
            );
        } else if self.has_editable() {
            lines.push(
                Line::from(vec![
                    "    ".into(),
                    key_hint::plain(KeyCode::Up).into(),
                    " select · ".into(),
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
            None,
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
        queue.set(vec!["From an older host".to_string()], Vec::new(), None);
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

    #[test]
    fn render_paused_after_an_interrupt() {
        let mut queue = queue_of(&["alpha", "dup", "dup"]);
        queue.paused = Some(nori_protocol::QueuePause::Interrupted);
        assert_snapshot!(
            "render_paused_after_an_interrupt",
            format!("{:?}", render(&queue, 72))
        );
    }

    #[test]
    fn render_paused_after_a_turn_error() {
        let mut queue = queue_of(&["alpha"]);
        queue.paused = Some(nori_protocol::QueuePause::TurnFailed);
        assert_snapshot!(
            "render_paused_after_a_turn_error",
            format!("{:?}", render(&queue, 72))
        );
    }

    #[test]
    fn render_focused_list_with_a_selected_row() {
        let mut queue = queue_of(&["alpha", "dup", "dup"]);
        assert!(queue.focus());
        queue.move_selection(-1);
        // crossterm names the Delete key "Fwd Del" on macOS and "Del"
        // elsewhere, so each platform keeps its own expected hint.
        #[cfg(target_os = "macos")]
        let name = "render_focused_list_with_a_selected_row_macos";
        #[cfg(not(target_os = "macos"))]
        let name = "render_focused_list_with_a_selected_row";
        assert_snapshot!(name, format!("{:?}", render(&queue, 72)));
    }

    #[test]
    fn render_after_the_selected_row_started() {
        let mut queue = queue_of(&["dup", "dup", "tail"]);
        assert!(queue.focus());
        queue.move_selection(-2);
        queue.set(
            vec!["dup".to_string(), "tail".to_string()],
            vec![
                nori_protocol::QueueEntry {
                    id: "evt-1".to_string(),
                    text: "dup".to_string(),
                    editable: true,
                },
                nori_protocol::QueueEntry {
                    id: "evt-2".to_string(),
                    text: "tail".to_string(),
                    editable: true,
                },
            ],
            None,
        );
        assert_snapshot!(
            "render_after_the_selected_row_started",
            format!("{:?}", render(&queue, 72))
        );
    }
}

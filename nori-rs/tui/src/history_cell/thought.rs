//! Agent thought text as its own history block.
//!
//! An ACP agent server streams its thoughts as `agent_thought_chunk` updates.
//! Consecutive chunks form one thought block, joined exactly as supplied; any
//! other agent output ends it. Live updates, replayed history, the transcript
//! overlay, and `nori attach` all render the block with [`ThoughtCell`], so
//! they show the same text once.

use super::HistoryCell;
use crate::markdown::append_markdown;
use crate::wrapping::RtOptions;
use crate::wrapping::word_wrap_lines;
use ratatui::prelude::*;
use ratatui::style::Stylize;
use unicode_width::UnicodeWidthStr;

/// The gutter that sets thought text apart from the answer.
const THOUGHT_GUTTER: &str = "│ ";

/// One complete thought block, rendered as dim italic Markdown behind a
/// vertical-bar gutter so it reads as an aside rather than as the answer.
#[derive(Debug)]
pub(crate) struct ThoughtCell {
    text: String,
}

impl ThoughtCell {
    /// Returns `None` for a block with no visible text.
    pub(crate) fn new(text: String) -> Option<Self> {
        let text = text.trim();
        (!text.is_empty()).then(|| Self {
            text: text.to_string(),
        })
    }
}

impl HistoryCell for ThoughtCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut markdown = Vec::new();
        append_markdown(
            &self.text,
            Some(usize::from(width).saturating_sub(THOUGHT_GUTTER.width())),
            &mut markdown,
        );
        let thought_style = Style::default().dim().italic();
        let styled = markdown
            .into_iter()
            .map(|mut line| {
                line.spans = line
                    .spans
                    .into_iter()
                    .map(|span| span.patch_style(thought_style))
                    .collect();
                line
            })
            .collect::<Vec<_>>();
        let mut lines = word_wrap_lines(
            &styled,
            RtOptions::new(usize::from(width))
                .initial_indent(THOUGHT_GUTTER.dim().into())
                .subsequent_indent(THOUGHT_GUTTER.dim().into()),
        );
        for line in &mut lines {
            if line
                .spans
                .iter()
                .skip(1)
                .all(|span| span.content.is_empty())
            {
                *line = Line::from(THOUGHT_GUTTER.trim_end().dim());
            }
        }
        lines
    }
}

//! Finds where a streamed markdown source can be cut so the part before the cut never has to be
//! rendered again.
//!
//! A cut at line start `B` is usable when rendering the whole source equals rendering
//! `source[..B]` and `source[B..]` separately and joining them, and when no text appended later can
//! change that. Two kinds of cut qualify:
//!
//! - **Between blocks.** `B` starts a top-level block (or a table/prose segment). Every block
//!   before it is closed for good, and the renderer joins the two sides with one blank line.
//! - **Inside a paragraph.** `B` continues a top-level paragraph whose text so far holds no
//!   character that could open an inline construct closed on a later line. The renderer emits each
//!   source line of such a paragraph as its own line, so the sides join with nothing between.
//!   A later setext underline would still turn the whole paragraph into a heading; the collector
//!   watches for that and falls back to its last cut between blocks.
//!
//! Each candidate is also checked against the current render before it is used, so a cut is only
//! taken when it reproduces today's output exactly.

use std::path::Path;

use pulldown_cmark::Event;
use pulldown_cmark::Options;
use pulldown_cmark::Parser;
use pulldown_cmark::Tag;
use ratatui::text::Line;

use crate::markdown;
use crate::markdown_render::scan_segments;

/// A usable cut, relative to the source it was found in.
pub(super) struct Split {
    /// Byte offset of the cut.
    pub(super) offset: usize,
    /// Number of lines `source[..offset]` renders to.
    pub(super) line_count: usize,
    /// Whether the full render places a blank line between the two sides.
    pub(super) blank_between: bool,
    /// The cut falls between two lines of one paragraph.
    pub(super) inside_paragraph: bool,
}

/// Characters that can open an inline construct (emphasis, code span, link, autolink, inline
/// HTML, strikethrough, escape) that a later line could close.
const INLINE_OPENERS: &[char] = &['*', '_', '`', '[', ']', '<', '~', '\\', '!', '&'];

/// True when a later line could be a setext underline that turns an open paragraph into a heading.
pub(super) fn contains_setext_underline(source: &str) -> bool {
    source.lines().any(|line| {
        let line = line.trim();
        !line.is_empty() && (line.chars().all(|c| c == '=') || line.chars().all(|c| c == '-'))
    })
}

/// The latest usable cut in `source`, whose full render is `rendered`.
pub(super) fn latest_split(
    source: &str,
    rendered: &[Line<'static>],
    width: Option<usize>,
    cwd: Option<&Path>,
) -> Option<Split> {
    let scan = scan_segments(source);
    let (offset, inside_paragraph) = scan.segments.iter().rev().find_map(|segment| {
        let mut latest = (segment.range.start > 0).then_some((segment.range.start, false));
        if !segment.is_table {
            let prose = &source[segment.range.clone()];
            let candidate = latest_prose_candidate(prose)
                .map(|(offset, inside)| (segment.range.start + offset, inside));
            latest = latest.max(candidate);
        }
        latest
            .filter(|(offset, _)| scan.clean_line_starts.binary_search(offset).is_ok())
            .filter(|(offset, _)| *offset < source.len())
    })?;

    let mut before = Vec::new();
    markdown::append_markdown_with_cwd(&source[..offset], width, cwd, &mut before);
    let mut after = Vec::new();
    markdown::append_markdown_with_cwd(&source[offset..], width, cwd, &mut after);
    if before.is_empty() || after.is_empty() {
        return None;
    }
    let blank_between = !inside_paragraph;
    let joined_len = before.len() + usize::from(blank_between) + after.len();
    let joins = joined_len == rendered.len()
        && rendered[..before.len()] == before[..]
        && (!blank_between || rendered[before.len()] == Line::default())
        && rendered[rendered.len() - after.len()..] == after[..];
    joins.then_some(Split {
        offset,
        line_count: before.len(),
        blank_between,
        inside_paragraph,
    })
}

/// The latest candidate cut inside one prose segment, as `(offset, inside_paragraph)`.
fn latest_prose_candidate(prose: &str) -> Option<(usize, bool)> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let mut depth = 0;
    let mut latest = None;
    for (event, range) in Parser::new_ext(prose, options).into_offset_iter() {
        let top_level_start = depth == 0 && matches!(event, Event::Start(_) | Event::Rule);
        if top_level_start {
            let line_start = prose[..range.start]
                .rfind('\n')
                .map_or(0, |index| index + 1);
            let leading = &prose[line_start..range.start];
            if leading.chars().all(|c| c == ' ') && !line_at(prose, line_start).contains('|') {
                latest = latest.max(Some((line_start, false)));
            }
            if matches!(event, Event::Start(Tag::Paragraph)) {
                latest = latest.max(latest_line_inside_paragraph(prose, range).map(|o| (o, true)));
            }
        }
        match event {
            Event::Start(_) => depth += 1,
            Event::End(_) => depth -= 1,
            Event::Text(_)
            | Event::Code(_)
            | Event::Html(_)
            | Event::InlineHtml(_)
            | Event::FootnoteReference(_)
            | Event::SoftBreak
            | Event::HardBreak
            | Event::Rule
            | Event::TaskListMarker(_) => {}
        }
    }
    latest
}

/// The latest line start inside the paragraph spanning `range` that continues it with plain text
/// and follows only plain text.
fn latest_line_inside_paragraph(prose: &str, range: std::ops::Range<usize>) -> Option<usize> {
    let text = &prose[range.clone()];
    let plain_len = text.find(INLINE_OPENERS).unwrap_or(text.len());
    let content_len = text.trim_end().len();
    text.match_indices('\n')
        .map(|(index, _)| index + 1)
        .filter(|start| *start <= plain_len && *start < content_len)
        .map(|start| range.start + start)
        .filter(|start| {
            let line = line_at(prose, *start);
            line.starts_with(char::is_alphabetic) && !line.contains('|')
        })
        .max()
}

fn line_at(source: &str, start: usize) -> &str {
    let rest = &source[start..];
    rest.find('\n').map_or(rest, |end| &rest[..end])
}

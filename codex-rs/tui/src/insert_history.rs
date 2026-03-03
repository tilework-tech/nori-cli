use std::fmt;
use std::io;
use std::io::Write;

use crate::wrapping::word_wrap_lines;
use crate::wrapping::word_wrap_lines_borrowed;
use crossterm::Command;
use crossterm::cursor::MoveTo;
use crossterm::queue;
use crossterm::style::Color as CColor;
use crossterm::style::Colors;
use crossterm::style::Print;
use crossterm::style::SetAttribute;
use crossterm::style::SetBackgroundColor;
use crossterm::style::SetColors;
use crossterm::style::SetForegroundColor;
use crossterm::terminal::Clear;
use crossterm::terminal::ClearType;
use ratatui::layout::Size;
use ratatui::prelude::Backend;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::text::Line;
use ratatui::text::Span;

/// Insert `lines` above the viewport using the terminal's backend writer
/// (avoids direct stdout references).
///
/// Returns `true` if lines were actually inserted, `false` if there was no
/// room above the viewport (area.top() == 0).
pub fn insert_history_lines<B>(
    terminal: &mut crate::custom_terminal::Terminal<B>,
    lines: Vec<Line>,
) -> io::Result<bool>
where
    B: Backend + Write,
{
    let screen_size = terminal.backend().size().unwrap_or(Size::new(0, 0));

    let mut area = terminal.viewport_area;
    let mut should_update_area = false;
    let last_cursor_pos = terminal.last_known_cursor_pos;
    let writer = terminal.backend_mut();

    // Pre-wrap lines using word-aware wrapping so terminal scrollback sees the same
    // formatting as the TUI. This avoids character-level hard wrapping by the terminal.
    let wrapped = word_wrap_lines_borrowed(&lines, area.width.max(1) as usize);
    let wrapped_lines = wrapped.len() as u16;
    let cursor_top = if area.bottom() < screen_size.height {
        // If the viewport is not at the bottom of the screen, scroll it down to make room.
        // Don't scroll it past the bottom of the screen.
        let scroll_amount = wrapped_lines.min(screen_size.height - area.bottom());

        // Emit ANSI to scroll the lower region (from the top of the viewport to the bottom
        // of the screen) downward by `scroll_amount` lines. We do this by:
        //   1) Limiting the scroll region to [area.top()+1 .. screen_height] (1-based bounds)
        //   2) Placing the cursor at the top margin of that region
        //   3) Emitting Reverse Index (RI, ESC M) `scroll_amount` times
        //   4) Resetting the scroll region back to full screen
        let top_1based = area.top() + 1; // Convert 0-based row to 1-based for DECSTBM
        queue!(writer, SetScrollRegion(top_1based..screen_size.height))?;
        queue!(writer, MoveTo(0, area.top()))?;
        for _ in 0..scroll_amount {
            // Reverse Index (RI): ESC M
            queue!(writer, Print("\x1bM"))?;
        }
        queue!(writer, ResetScrollRegion)?;

        let cursor_top = area.top().saturating_sub(1);
        area.y += scroll_amount;
        should_update_area = true;
        cursor_top
    } else {
        area.top().saturating_sub(1)
    };

    // No room above the viewport for history lines.
    if area.top() == 0 {
        tracing::warn!(
            "insert_history_lines: no room above viewport (area.top()==0), skipping {} lines",
            lines.len()
        );
        let _ = writer;
        return Ok(false);
    }

    // Limit the scroll region to the lines from the top of the screen to the
    // top of the viewport. With this in place, when we add lines inside this
    // area, only the lines in this area will be scrolled. We place the cursor
    // at the end of the scroll region, and add lines starting there.
    //
    // ┌─Screen───────────────────────┐
    // │┌╌Scroll region╌╌╌╌╌╌╌╌╌╌╌╌╌╌┐│
    // │┆                            ┆│
    // │┆                            ┆│
    // │┆                            ┆│
    // │█╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┘│
    // │╭─Viewport───────────────────╮│
    // ││                            ││
    // │╰────────────────────────────╯│
    // └──────────────────────────────┘
    queue!(writer, SetScrollRegion(1..area.top()))?;

    // NB: we are using MoveTo instead of set_cursor_position here to avoid messing with the
    // terminal's last_known_cursor_position, which hopefully will still be accurate after we
    // fetch/restore the cursor position. insert_history_lines should be cursor-position-neutral :)
    queue!(writer, MoveTo(0, cursor_top))?;

    for line in wrapped {
        queue!(writer, Print("\r\n"))?;
        queue!(
            writer,
            SetColors(Colors::new(
                line.style
                    .fg
                    .map(std::convert::Into::into)
                    .unwrap_or(CColor::Reset),
                line.style
                    .bg
                    .map(std::convert::Into::into)
                    .unwrap_or(CColor::Reset)
            ))
        )?;
        queue!(writer, Clear(ClearType::UntilNewLine))?;
        // Merge line-level style into each span so that ANSI colors reflect
        // line styles (e.g., blockquotes with green fg).
        let merged_spans: Vec<Span> = line
            .spans
            .iter()
            .map(|s| Span {
                style: s.style.patch(line.style),
                content: s.content.clone(),
            })
            .collect();
        write_spans(writer, merged_spans.iter())?;
    }

    queue!(writer, ResetScrollRegion)?;

    // Restore the cursor position to where it was before we started.
    queue!(writer, MoveTo(last_cursor_pos.x, last_cursor_pos.y))?;

    let _ = writer;
    if should_update_area {
        terminal.set_viewport_area(area);
    }

    Ok(true)
}

/// Write pending history lines directly to terminal positions above the viewport,
/// without using scroll regions. This avoids pushing stale content into the
/// terminal scrollback when the viewport has just shrunk from full-screen.
///
/// Lines are bottom-aligned within the available rows: the last consumed line
/// appears immediately above the viewport. Rows above the written lines are
/// cleared.
///
/// Returns the number of screen rows written. Lines that were successfully
/// written are drained from `lines`.
pub fn write_pending_lines_directly<B>(
    terminal: &mut crate::custom_terminal::Terminal<B>,
    lines: &mut Vec<Line<'static>>,
    available_rows: u16,
) -> io::Result<u16>
where
    B: Backend + Write,
{
    if available_rows == 0 || lines.is_empty() {
        return Ok(0);
    }

    let width = terminal.viewport_area.width.max(1) as usize;

    // First pass: figure out how many original lines fit by wrapping each
    // individually and counting screen rows.
    let mut total_rows: u16 = 0;
    let mut lines_consumed: usize = 0;
    for line in lines.iter() {
        let wrapped_count = word_wrap_lines(std::iter::once(line.clone()), width).len() as u16;
        if total_rows + wrapped_count > available_rows {
            break;
        }
        total_rows += wrapped_count;
        lines_consumed += 1;
    }

    if lines_consumed == 0 {
        return Ok(0);
    }

    // Drain consumed lines and wrap them as a batch for writing.
    let consumed: Vec<Line<'static>> = lines.drain(..lines_consumed).collect();
    let wrapped = word_wrap_lines(consumed, width);

    // Bottom-align: start writing from (available_rows - total_rows).
    let start_row = available_rows - total_rows;

    let last_cursor_pos = terminal.last_known_cursor_pos;
    let writer = terminal.backend_mut();

    // Clear any stale rows above the written content.
    for row in 0..start_row {
        queue!(writer, MoveTo(0, row))?;
        queue!(writer, Clear(ClearType::UntilNewLine))?;
    }

    // Write the wrapped lines directly to their target positions.
    for (i, line) in wrapped.iter().enumerate() {
        let row = start_row + i as u16;
        queue!(writer, MoveTo(0, row))?;
        queue!(
            writer,
            SetColors(Colors::new(
                line.style
                    .fg
                    .map(std::convert::Into::into)
                    .unwrap_or(CColor::Reset),
                line.style
                    .bg
                    .map(std::convert::Into::into)
                    .unwrap_or(CColor::Reset)
            ))
        )?;
        queue!(writer, Clear(ClearType::UntilNewLine))?;
        let merged_spans: Vec<Span> = line
            .spans
            .iter()
            .map(|s| Span {
                style: s.style.patch(line.style),
                content: s.content.clone(),
            })
            .collect();
        write_spans(writer, merged_spans.iter())?;
    }

    // Restore cursor position.
    queue!(writer, MoveTo(last_cursor_pos.x, last_cursor_pos.y))?;

    Ok(total_rows)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetScrollRegion(pub std::ops::Range<u16>);

impl Command for SetScrollRegion {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[{};{}r", self.0.start, self.0.end)
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> std::io::Result<()> {
        panic!("tried to execute SetScrollRegion command using WinAPI, use ANSI instead");
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        // TODO(nornagon): is this supported on Windows?
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResetScrollRegion;

impl Command for ResetScrollRegion {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[r")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> std::io::Result<()> {
        panic!("tried to execute ResetScrollRegion command using WinAPI, use ANSI instead");
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        // TODO(nornagon): is this supported on Windows?
        true
    }
}

struct ModifierDiff {
    pub from: Modifier,
    pub to: Modifier,
}

impl ModifierDiff {
    fn queue<W>(self, mut w: W) -> io::Result<()>
    where
        W: io::Write,
    {
        use crossterm::style::Attribute as CAttribute;
        let removed = self.from - self.to;
        if removed.contains(Modifier::REVERSED) {
            queue!(w, SetAttribute(CAttribute::NoReverse))?;
        }
        if removed.contains(Modifier::BOLD) {
            queue!(w, SetAttribute(CAttribute::NormalIntensity))?;
            if self.to.contains(Modifier::DIM) {
                queue!(w, SetAttribute(CAttribute::Dim))?;
            }
        }
        if removed.contains(Modifier::ITALIC) {
            queue!(w, SetAttribute(CAttribute::NoItalic))?;
        }
        if removed.contains(Modifier::UNDERLINED) {
            queue!(w, SetAttribute(CAttribute::NoUnderline))?;
        }
        if removed.contains(Modifier::DIM) {
            queue!(w, SetAttribute(CAttribute::NormalIntensity))?;
        }
        if removed.contains(Modifier::CROSSED_OUT) {
            queue!(w, SetAttribute(CAttribute::NotCrossedOut))?;
        }
        if removed.contains(Modifier::SLOW_BLINK) || removed.contains(Modifier::RAPID_BLINK) {
            queue!(w, SetAttribute(CAttribute::NoBlink))?;
        }

        let added = self.to - self.from;
        if added.contains(Modifier::REVERSED) {
            queue!(w, SetAttribute(CAttribute::Reverse))?;
        }
        if added.contains(Modifier::BOLD) {
            queue!(w, SetAttribute(CAttribute::Bold))?;
        }
        if added.contains(Modifier::ITALIC) {
            queue!(w, SetAttribute(CAttribute::Italic))?;
        }
        if added.contains(Modifier::UNDERLINED) {
            queue!(w, SetAttribute(CAttribute::Underlined))?;
        }
        if added.contains(Modifier::DIM) {
            queue!(w, SetAttribute(CAttribute::Dim))?;
        }
        if added.contains(Modifier::CROSSED_OUT) {
            queue!(w, SetAttribute(CAttribute::CrossedOut))?;
        }
        if added.contains(Modifier::SLOW_BLINK) {
            queue!(w, SetAttribute(CAttribute::SlowBlink))?;
        }
        if added.contains(Modifier::RAPID_BLINK) {
            queue!(w, SetAttribute(CAttribute::RapidBlink))?;
        }

        Ok(())
    }
}

fn write_spans<'a, I>(mut writer: &mut impl Write, content: I) -> io::Result<()>
where
    I: IntoIterator<Item = &'a Span<'a>>,
{
    let mut fg = Color::Reset;
    let mut bg = Color::Reset;
    let mut last_modifier = Modifier::empty();
    for span in content {
        let mut modifier = Modifier::empty();
        modifier.insert(span.style.add_modifier);
        modifier.remove(span.style.sub_modifier);
        if modifier != last_modifier {
            let diff = ModifierDiff {
                from: last_modifier,
                to: modifier,
            };
            diff.queue(&mut writer)?;
            last_modifier = modifier;
        }
        let next_fg = span.style.fg.unwrap_or(Color::Reset);
        let next_bg = span.style.bg.unwrap_or(Color::Reset);
        if next_fg != fg || next_bg != bg {
            queue!(
                writer,
                SetColors(Colors::new(next_fg.into(), next_bg.into()))
            )?;
            fg = next_fg;
            bg = next_bg;
        }

        queue!(writer, Print(span.content.clone()))?;
    }

    queue!(
        writer,
        SetForegroundColor(CColor::Reset),
        SetBackgroundColor(CColor::Reset),
        SetAttribute(crossterm::style::Attribute::Reset),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown_render::render_markdown_text;
    use crate::test_backend::VT100Backend;
    use ratatui::layout::Rect;
    use ratatui::style::Color;

    #[test]
    fn writes_bold_then_regular_spans() {
        use ratatui::style::Stylize;

        let spans = ["A".bold(), "B".into()];

        let mut actual: Vec<u8> = Vec::new();
        write_spans(&mut actual, spans.iter()).unwrap();

        let mut expected: Vec<u8> = Vec::new();
        queue!(
            expected,
            SetAttribute(crossterm::style::Attribute::Bold),
            Print("A"),
            SetAttribute(crossterm::style::Attribute::NormalIntensity),
            Print("B"),
            SetForegroundColor(CColor::Reset),
            SetBackgroundColor(CColor::Reset),
            SetAttribute(crossterm::style::Attribute::Reset),
        )
        .unwrap();

        assert_eq!(
            String::from_utf8(actual).unwrap(),
            String::from_utf8(expected).unwrap()
        );
    }

    #[test]
    fn vt100_blockquote_line_emits_green_fg() {
        // Set up a small off-screen terminal
        let width: u16 = 40;
        let height: u16 = 10;
        let backend = VT100Backend::new(width, height);
        let mut term = crate::custom_terminal::Terminal::with_options(backend).expect("terminal");
        // Place viewport on the last line so history inserts scroll upward
        let viewport = Rect::new(0, height - 1, width, 1);
        term.set_viewport_area(viewport);

        // Build a blockquote-like line: apply line-level green style and prefix "> "
        let mut line: Line<'static> = Line::from(vec!["> ".into(), "Hello world".into()]);
        line = line.style(Color::Green);
        insert_history_lines(&mut term, vec![line])
            .expect("Failed to insert history lines in test");

        let mut saw_colored = false;
        'outer: for row in 0..height {
            for col in 0..width {
                if let Some(cell) = term.backend().vt100().screen().cell(row, col)
                    && cell.has_contents()
                    && cell.fgcolor() != vt100::Color::Default
                {
                    saw_colored = true;
                    break 'outer;
                }
            }
        }
        assert!(
            saw_colored,
            "expected at least one colored cell in vt100 output"
        );
    }

    #[test]
    fn vt100_blockquote_wrap_preserves_color_on_all_wrapped_lines() {
        // Force wrapping by using a narrow viewport width and a long blockquote line.
        let width: u16 = 20;
        let height: u16 = 8;
        let backend = VT100Backend::new(width, height);
        let mut term = crate::custom_terminal::Terminal::with_options(backend).expect("terminal");
        // Viewport is the last line so history goes directly above it.
        let viewport = Rect::new(0, height - 1, width, 1);
        term.set_viewport_area(viewport);

        // Create a long blockquote with a distinct prefix and enough text to wrap.
        let mut line: Line<'static> = Line::from(vec![
            "> ".into(),
            "This is a long quoted line that should wrap".into(),
        ]);
        line = line.style(Color::Green);

        insert_history_lines(&mut term, vec![line])
            .expect("Failed to insert history lines in test");

        // Parse and inspect the final screen buffer.
        let screen = term.backend().vt100().screen();

        // Collect rows that are non-empty; these should correspond to our wrapped lines.
        let mut non_empty_rows: Vec<u16> = Vec::new();
        for row in 0..height {
            let mut any = false;
            for col in 0..width {
                if let Some(cell) = screen.cell(row, col)
                    && cell.has_contents()
                    && cell.contents() != "\0"
                    && cell.contents() != " "
                {
                    any = true;
                    break;
                }
            }
            if any {
                non_empty_rows.push(row);
            }
        }

        // Expect at least two rows due to wrapping.
        assert!(
            non_empty_rows.len() >= 2,
            "expected wrapped output to span >=2 rows, got {non_empty_rows:?}",
        );

        // For each non-empty row, ensure all non-space cells are using a non-default fg color.
        for row in non_empty_rows {
            for col in 0..width {
                if let Some(cell) = screen.cell(row, col) {
                    let contents = cell.contents();
                    if !contents.is_empty() && contents != " " {
                        assert!(
                            cell.fgcolor() != vt100::Color::Default,
                            "expected non-default fg on row {row} col {col}, got {:?}",
                            cell.fgcolor()
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn vt100_colored_prefix_then_plain_text_resets_color() {
        let width: u16 = 40;
        let height: u16 = 6;
        let backend = VT100Backend::new(width, height);
        let mut term = crate::custom_terminal::Terminal::with_options(backend).expect("terminal");
        let viewport = Rect::new(0, height - 1, width, 1);
        term.set_viewport_area(viewport);

        // First span colored, rest plain.
        let line: Line<'static> = Line::from(vec![
            Span::styled("1. ", ratatui::style::Style::default().fg(Color::LightBlue)),
            Span::raw("Hello world"),
        ]);

        insert_history_lines(&mut term, vec![line])
            .expect("Failed to insert history lines in test");

        let screen = term.backend().vt100().screen();

        // Find the first non-empty row; verify first three cells are colored, following cells default.
        'rows: for row in 0..height {
            let mut has_text = false;
            for col in 0..width {
                if let Some(cell) = screen.cell(row, col)
                    && cell.has_contents()
                    && cell.contents() != " "
                {
                    has_text = true;
                    break;
                }
            }
            if !has_text {
                continue;
            }

            // Expect "1. Hello world" starting at col 0.
            for col in 0..3 {
                let cell = screen.cell(row, col).unwrap();
                assert!(
                    cell.fgcolor() != vt100::Color::Default,
                    "expected colored prefix at col {col}, got {:?}",
                    cell.fgcolor()
                );
            }
            for col in 3..(3 + "Hello world".len() as u16) {
                let cell = screen.cell(row, col).unwrap();
                assert_eq!(
                    cell.fgcolor(),
                    vt100::Color::Default,
                    "expected default color for plain text at col {col}, got {:?}",
                    cell.fgcolor()
                );
            }
            break 'rows;
        }
    }

    #[test]
    fn vt100_deep_nested_mixed_list_third_level_marker_is_colored() {
        // Markdown with five levels (ordered → unordered → ordered → unordered → unordered).
        let md = "1. First\n   - Second level\n     1. Third level (ordered)\n        - Fourth level (bullet)\n          - Fifth level to test indent consistency\n";
        let text = render_markdown_text(md);
        let lines: Vec<Line<'static>> = text.lines.clone();

        let width: u16 = 60;
        let height: u16 = 12;
        let backend = VT100Backend::new(width, height);
        let mut term = crate::custom_terminal::Terminal::with_options(backend).expect("terminal");
        let viewport = ratatui::layout::Rect::new(0, height - 1, width, 1);
        term.set_viewport_area(viewport);

        insert_history_lines(&mut term, lines).expect("Failed to insert history lines in test");

        let screen = term.backend().vt100().screen();

        // Reconstruct screen rows as strings to locate the 3rd level line.
        let rows: Vec<String> = screen.rows(0, width).collect();

        let needle = "1. Third level (ordered)";
        let row_idx = rows
            .iter()
            .position(|r| r.contains(needle))
            .unwrap_or_else(|| {
                panic!("expected to find row containing {needle:?}, have rows: {rows:?}")
            });
        let col_start = rows[row_idx].find(needle).unwrap() as u16; // column where '1' starts

        // Verify that the numeric marker ("1.") at the third level is colored
        // (non-default fg) and the content after the following space resets to default.
        for c in [col_start, col_start + 1] {
            let cell = screen.cell(row_idx as u16, c).unwrap();
            assert!(
                cell.fgcolor() != vt100::Color::Default,
                "expected colored 3rd-level marker at row {row_idx} col {c}, got {:?}",
                cell.fgcolor()
            );
        }
        let content_col = col_start + 3; // skip '1', '.', and the space
        if let Some(cell) = screen.cell(row_idx as u16, content_col) {
            assert_eq!(
                cell.fgcolor(),
                vt100::Color::Default,
                "expected default color for 3rd-level content at row {row_idx} col {content_col}, got {:?}",
                cell.fgcolor()
            );
        }
    }

    /// When the viewport occupies the entire screen (area.top() == 0), there is
    /// no room above the viewport. insert_history_lines must not corrupt the
    /// viewport content by writing through a degenerate scroll region.
    #[test]
    fn full_screen_viewport_does_not_corrupt_display() {
        let width: u16 = 40;
        let height: u16 = 10;
        let backend = VT100Backend::new(width, height);
        let mut term = crate::custom_terminal::Terminal::with_options(backend).expect("terminal");

        // Viewport fills the entire screen: y=0, height=10
        let viewport = Rect::new(0, 0, width, height);
        term.set_viewport_area(viewport);

        // Draw some known content into the viewport first
        term.draw(|frame| {
            let buf = frame.buffer_mut();
            for y in 0..height {
                let text = format!("Row {y}");
                buf.set_string(0, y, &text, ratatui::style::Style::default());
            }
        })
        .expect("draw");
        // Flush the draw output so vt100 sees it
        Backend::flush(term.backend_mut()).expect("flush");

        // Capture the screen contents before insert_history_lines
        let before: Vec<String> = term.backend().vt100().screen().rows(0, width).collect();

        // Now try to insert history lines — there's no room above the viewport
        let line = Line::from("This should not corrupt the display");
        insert_history_lines(&mut term, vec![line]).expect("insert");
        Backend::flush(term.backend_mut()).expect("flush");

        // The viewport content must be unchanged
        let after: Vec<String> = term.backend().vt100().screen().rows(0, width).collect();

        pretty_assertions::assert_eq!(
            before,
            after,
            "viewport content was corrupted by insert_history_lines when area.top()==0"
        );
    }

    /// insert_history_lines must return false when area.top() == 0
    /// so callers know the lines were NOT inserted and can retain them.
    #[test]
    fn full_screen_viewport_returns_false() {
        let width: u16 = 40;
        let height: u16 = 10;
        let backend = VT100Backend::new(width, height);
        let mut term = crate::custom_terminal::Terminal::with_options(backend).expect("terminal");

        let viewport = Rect::new(0, 0, width, height);
        term.set_viewport_area(viewport);

        let line = Line::from("This line has no room");
        let inserted = insert_history_lines(&mut term, vec![line]).expect("insert");
        assert!(
            !inserted,
            "insert_history_lines should return false when area.top() == 0"
        );
    }

    /// When viewport was at y=0 (full screen) and then shrinks, repositioning
    /// the viewport to the bottom of the screen should restore the ability to
    /// insert history lines.
    #[test]
    fn history_insertion_works_after_viewport_repositioned_from_y0() {
        let width: u16 = 40;
        let screen_height: u16 = 20;
        let backend = VT100Backend::new(width, screen_height);
        let mut term = crate::custom_terminal::Terminal::with_options(backend).expect("terminal");

        // Phase 1: viewport fills the entire screen (simulating large active cell)
        let full_viewport = Rect::new(0, 0, width, screen_height);
        term.set_viewport_area(full_viewport);

        // Phase 2: widget shrinks (active cell completed/flushed).
        // Simulate the fix: reposition viewport to bottom of screen.
        let small_height: u16 = 8;
        let repositioned = Rect::new(0, screen_height - small_height, width, small_height);
        term.set_viewport_area(repositioned);

        // Insert a history line — it should succeed now.
        let line = Line::from("Recovered history entry");
        let inserted =
            insert_history_lines(&mut term, vec![line]).expect("insert after reposition");
        assert!(
            inserted,
            "insert_history_lines should succeed after viewport repositioned"
        );
        Backend::flush(term.backend_mut()).expect("flush");

        let rows: Vec<String> = term.backend().vt100().screen().rows(0, width).collect();
        let found = rows.iter().any(|r| r.contains("Recovered history entry"));
        assert!(
            found,
            "history line should appear on screen after viewport recovery, got rows: {rows:?}"
        );
    }

    /// When there IS room above the viewport, history lines should appear
    /// above the viewport and the viewport content should be preserved.
    #[test]
    fn history_lines_inserted_above_viewport_with_room() {
        let width: u16 = 40;
        let height: u16 = 10;
        let viewport_y: usize = 5;
        let viewport_h: usize = 5;
        let backend = VT100Backend::new(width, height);
        let mut term = crate::custom_terminal::Terminal::with_options(backend).expect("terminal");

        // Viewport at y=5 with height=5, leaving 5 rows above for history
        let viewport = Rect::new(0, viewport_y as u16, width, viewport_h as u16);
        term.set_viewport_area(viewport);

        // Draw known viewport content
        term.draw(|frame| {
            let area = frame.area();
            let buf = frame.buffer_mut();
            for i in 0..area.height {
                let text = format!("Viewport row {i}");
                buf.set_string(area.x, area.y + i, &text, ratatui::style::Style::default());
            }
        })
        .expect("draw");
        Backend::flush(term.backend_mut()).expect("flush");

        // Insert a history line
        let line = Line::from("History entry");
        insert_history_lines(&mut term, vec![line]).expect("insert");
        Backend::flush(term.backend_mut()).expect("flush");

        let rows: Vec<String> = term.backend().vt100().screen().rows(0, width).collect();

        // The history line should appear somewhere above the viewport
        let history_row = rows
            .iter()
            .position(|r| r.contains("History entry"))
            .unwrap_or_else(|| {
                panic!("expected 'History entry' above viewport, got rows: {rows:?}")
            });
        assert!(
            history_row < viewport_y,
            "history line at row {history_row} should be above viewport at y={viewport_y}",
        );

        // Viewport content should still be intact — find it by searching for
        // "Viewport row 0" and checking consecutive rows from there.
        let vp_start = rows
            .iter()
            .position(|r| r.contains("Viewport row 0"))
            .expect("could not find 'Viewport row 0' on screen");
        for i in 0..viewport_h {
            let row_text = &rows[vp_start + i];
            assert!(
                row_text.contains(&format!("Viewport row {i}")),
                "viewport row {i} should contain 'Viewport row {i}', got: {row_text:?}"
            );
        }
    }

    /// After a full-screen viewport shrinks and is repositioned, calling
    /// write_pending_lines_directly should place history lines in the vacated
    /// rows (above the viewport), NOT leave stale viewport content behind.
    #[test]
    fn direct_write_replaces_stale_content_after_viewport_shrink() {
        let width: u16 = 40;
        let screen_height: u16 = 20;
        let backend = VT100Backend::new(width, screen_height);
        let mut term = crate::custom_terminal::Terminal::with_options(backend).expect("terminal");

        // Phase 1: viewport fills the entire screen, draw stale content.
        let full_viewport = Rect::new(0, 0, width, screen_height);
        term.set_viewport_area(full_viewport);
        term.draw(|frame| {
            let buf = frame.buffer_mut();
            for y in 0..screen_height {
                buf.set_string(
                    0,
                    y,
                    format!("Stale row {y}"),
                    ratatui::style::Style::default(),
                );
            }
        })
        .expect("draw");
        Backend::flush(term.backend_mut()).expect("flush");

        // Phase 2: viewport shrinks, reposition to bottom.
        let small_height: u16 = 8;
        let new_y = screen_height - small_height;
        let repositioned = Rect::new(0, new_y, width, small_height);
        term.set_viewport_area(repositioned);

        // Write pending history lines directly into the vacated area.
        let mut pending = vec![
            Line::from("History A"),
            Line::from("History B"),
            Line::from("History C"),
        ];
        let rows_written =
            write_pending_lines_directly(&mut term, &mut pending, new_y).expect("direct write");
        Backend::flush(term.backend_mut()).expect("flush");

        pretty_assertions::assert_eq!(rows_written, 3, "should have written 3 rows");
        pretty_assertions::assert_eq!(pending.len(), 0, "all lines should be consumed");

        // Verify: the vacated area should contain history, not stale content.
        let rows: Vec<String> = term.backend().vt100().screen().rows(0, width).collect();
        let vacated = &rows[..new_y as usize];
        for row_text in vacated {
            assert!(
                !row_text.contains("Stale"),
                "vacated row should not contain stale content, got: {row_text:?}"
            );
        }
        assert!(
            vacated.iter().any(|r| r.contains("History A")),
            "expected 'History A' in vacated area, got: {vacated:?}"
        );
        assert!(
            vacated.iter().any(|r| r.contains("History B")),
            "expected 'History B' in vacated area, got: {vacated:?}"
        );
        assert!(
            vacated.iter().any(|r| r.contains("History C")),
            "expected 'History C' in vacated area, got: {vacated:?}"
        );
    }

    /// When there are more pending lines than available rows,
    /// write_pending_lines_directly should write as many as fit
    /// and leave the rest in the pending vector.
    #[test]
    fn direct_write_partial_when_more_lines_than_rows() {
        let width: u16 = 40;
        let screen_height: u16 = 10;
        let backend = VT100Backend::new(width, screen_height);
        let mut term = crate::custom_terminal::Terminal::with_options(backend).expect("terminal");

        // Viewport at bottom with only 3 rows of vacated space above.
        let viewport_height: u16 = 7;
        let new_y = screen_height - viewport_height; // 3
        let viewport = Rect::new(0, new_y, width, viewport_height);
        term.set_viewport_area(viewport);

        let mut pending = vec![
            Line::from("Line 1"),
            Line::from("Line 2"),
            Line::from("Line 3"),
            Line::from("Line 4"),
            Line::from("Line 5"),
        ];
        let rows_written =
            write_pending_lines_directly(&mut term, &mut pending, new_y).expect("direct write");
        Backend::flush(term.backend_mut()).expect("flush");

        pretty_assertions::assert_eq!(rows_written, 3, "should write exactly 3 rows");
        pretty_assertions::assert_eq!(pending.len(), 2, "2 lines should remain unconsumed");
        pretty_assertions::assert_eq!(pending[0], Line::from("Line 4"));
        pretty_assertions::assert_eq!(pending[1], Line::from("Line 5"));

        // Verify the first 3 lines appear in the vacated area.
        let rows: Vec<String> = term.backend().vt100().screen().rows(0, width).collect();
        assert!(rows[0].contains("Line 1"), "row 0: {}", rows[0]);
        assert!(rows[1].contains("Line 2"), "row 1: {}", rows[1]);
        assert!(rows[2].contains("Line 3"), "row 2: {}", rows[2]);
    }

    /// write_pending_lines_directly must handle word wrapping correctly:
    /// a long line that wraps to multiple rows should count all wrapped
    /// rows toward the available space.
    #[test]
    fn direct_write_accounts_for_word_wrapping() {
        let width: u16 = 20;
        let screen_height: u16 = 10;
        let backend = VT100Backend::new(width, screen_height);
        let mut term = crate::custom_terminal::Terminal::with_options(backend).expect("terminal");

        let viewport_height: u16 = 4;
        let new_y = screen_height - viewport_height; // 6 rows available
        let viewport = Rect::new(0, new_y, width, viewport_height);
        term.set_viewport_area(viewport);

        // "Short" fits in 1 row. The long line wraps to ~3 rows at width=20.
        // Together they need ~4 rows, which fits in the 6 available.
        let mut pending = vec![
            Line::from("Short"),
            Line::from("This is a long line that should wrap to multiple rows"),
        ];
        let rows_written =
            write_pending_lines_directly(&mut term, &mut pending, new_y).expect("direct write");
        Backend::flush(term.backend_mut()).expect("flush");

        // "Short" = 1 row + 53-char line wraps to 3 rows at width=20 = 4 total.
        pretty_assertions::assert_eq!(rows_written, 4);
        pretty_assertions::assert_eq!(pending.len(), 0, "all lines should be consumed");

        let rows: Vec<String> = term.backend().vt100().screen().rows(0, width).collect();
        let vacated = &rows[..new_y as usize];
        assert!(
            vacated.iter().any(|r| r.contains("Short")),
            "expected 'Short' in vacated area, got: {vacated:?}"
        );
        assert!(
            vacated.iter().any(|r| r.contains("long line")),
            "expected part of wrapped line in vacated area, got: {vacated:?}"
        );
    }

    /// When a single pending line wraps to more rows than available,
    /// write_pending_lines_directly should not write it (it doesn't fit)
    /// and return 0 rows written.
    #[test]
    fn direct_write_skips_line_too_tall_for_available_space() {
        let width: u16 = 10;
        let screen_height: u16 = 10;
        let backend = VT100Backend::new(width, screen_height);
        let mut term = crate::custom_terminal::Terminal::with_options(backend).expect("terminal");

        let viewport_height: u16 = 8;
        let new_y = screen_height - viewport_height; // 2 rows available
        let viewport = Rect::new(0, new_y, width, viewport_height);
        term.set_viewport_area(viewport);

        // This line wraps to way more than 2 rows at width=10.
        let mut pending = vec![Line::from(
            "This is a very long line that will wrap to many rows at width ten",
        )];
        let rows_written =
            write_pending_lines_directly(&mut term, &mut pending, new_y).expect("direct write");

        pretty_assertions::assert_eq!(rows_written, 0, "line too tall, nothing should be written");
        pretty_assertions::assert_eq!(pending.len(), 1, "line should remain unconsumed");
    }
}

use std::fmt;
use std::io;
use std::io::Write;

use crate::wrapping::adaptive_wrap_line;
use crate::wrapping::line_contains_url_like;
use crate::wrapping::line_has_mixed_url_and_non_url_tokens;
use crossterm::Command;
use crossterm::cursor::MoveDown;
use crossterm::cursor::MoveTo;
use crossterm::cursor::MoveToColumn;
use crossterm::cursor::RestorePosition;
use crossterm::cursor::SavePosition;
use crossterm::queue;
use crossterm::style::Color as CColor;
use crossterm::style::Print;
use crossterm::style::SetAttribute;
use crossterm::style::SetBackgroundColor;
use crossterm::style::SetForegroundColor;
use crossterm::terminal::Clear;
use crossterm::terminal::ClearType;
use ratatui::layout::Size;
use ratatui::prelude::Backend;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::text::Line;
use ratatui::text::Span;

fn queue_colors(writer: &mut impl Write, fg: Color, bg: Color) -> io::Result<()> {
    queue!(writer, SetForegroundColor(fg.into()))?;
    queue!(writer, SetBackgroundColor(bg.into()))
}

fn merged_line_spans(line: &Line<'_>) -> Vec<Span<'static>> {
    line.spans
        .iter()
        .map(|span| Span {
            style: span.style.patch(line.style),
            content: span.content.to_string().into(),
        })
        .collect()
}

fn wrap_history_line<'a>(line: &'a Line<'a>, width: usize) -> Vec<Line<'a>> {
    if line_contains_url_like(line) && !line_has_mixed_url_and_non_url_tokens(line) {
        vec![line.clone()]
    } else {
        adaptive_wrap_line(line, width)
    }
}

fn physical_row_count(line: &Line<'_>, width: usize) -> usize {
    line.width().max(1).div_ceil(width.max(1))
}

/// Insert `lines` above the viewport using the terminal's backend writer
/// (avoids direct stdout references).
///
/// Returns `true` if lines were actually inserted, `false` if there was no
/// usable room above the viewport (area.top() <= 1; scroll-region insertion
/// needs at least two rows above the viewport to form a valid DECSTBM region).
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

    let wrap_width = area.width.max(1) as usize;
    let mut wrapped = Vec::new();
    let mut wrapped_rows = 0usize;
    for line in &lines {
        let line_wrapped = wrap_history_line(line, wrap_width);
        wrapped_rows += line_wrapped
            .iter()
            .map(|line| physical_row_count(line, wrap_width))
            .sum::<usize>();
        wrapped.extend(line_wrapped);
    }
    let wrapped_lines = wrapped_rows as u16;
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

    // No usable room above the viewport for history lines. Scroll-region
    // insertion needs a region spanning at least rows 1..2 (1-based): DECSTBM
    // requires the bottom margin to exceed the top margin, so a region ending
    // at row 1 (area.top() <= 1) is degenerate and terminals fall back to a
    // full-screen scroll region, scrolling the viewport itself.
    if area.top() <= 1 {
        tracing::warn!(
            "insert_history_lines: no usable room above viewport (area.top()=={}), skipping {} lines",
            area.top(),
            lines.len()
        );
        // The scroll-down branch above may have moved the viewport and the
        // cursor; keep the call cursor-position-neutral and persist the moved
        // viewport so terminal state stays consistent. Without that branch
        // nothing was queued, so stay byte-silent.
        if should_update_area {
            queue!(writer, MoveTo(last_cursor_pos.x, last_cursor_pos.y))?;
            terminal.set_viewport_area(area);
        }
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
        let physical_rows = physical_row_count(&line, wrap_width);
        if physical_rows > 1 {
            queue!(writer, SavePosition)?;
            for _ in 1..physical_rows {
                queue!(writer, MoveDown(1), MoveToColumn(0))?;
                queue!(writer, Clear(ClearType::UntilNewLine))?;
            }
            queue!(writer, RestorePosition)?;
        }
        queue_colors(
            writer,
            line.style.fg.unwrap_or(Color::Reset),
            line.style.bg.unwrap_or(Color::Reset),
        )?;
        queue!(writer, Clear(ClearType::UntilNewLine))?;
        // Merge line-level style into each span so that ANSI colors reflect
        // line styles (e.g., blockquotes with green fg).
        let merged_spans = merged_line_spans(&line);
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

    // First pass: figure out how many original lines fit, including terminal
    // soft-wrap rows used to preserve long URL tokens.
    let mut total_rows: u16 = 0;
    let mut lines_consumed: usize = 0;
    for line in lines.iter() {
        let wrapped_count = wrap_history_line(line, width)
            .iter()
            .map(|line| physical_row_count(line, width))
            .sum::<usize>()
            .try_into()
            .unwrap_or(u16::MAX);
        if total_rows + wrapped_count > available_rows {
            break;
        }
        total_rows += wrapped_count;
        lines_consumed += 1;
    }

    if lines_consumed == 0 {
        return Ok(0);
    }

    // Drain consumed lines and wrap them for writing.
    let consumed: Vec<Line<'static>> = lines.drain(..lines_consumed).collect();
    let wrapped = consumed
        .iter()
        .flat_map(|line| wrap_history_line(line, width))
        .collect::<Vec<_>>();

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
    let mut row = start_row;
    for line in &wrapped {
        let physical_rows = physical_row_count(line, width) as u16;
        for offset in 0..physical_rows {
            queue!(writer, MoveTo(0, row + offset))?;
            queue!(writer, Clear(ClearType::UntilNewLine))?;
        }
        queue!(writer, MoveTo(0, row))?;
        queue_colors(
            writer,
            line.style.fg.unwrap_or(Color::Reset),
            line.style.bg.unwrap_or(Color::Reset),
        )?;
        queue!(writer, Clear(ClearType::UntilNewLine))?;
        let merged_spans = merged_line_spans(line);
        write_spans(writer, merged_spans.iter())?;
        row += physical_rows;
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
            queue_colors(&mut writer, next_fg, next_bg)?;
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
    use crate::history_cell::AgentMessageCell;
    use crate::history_cell::HistoryCell;
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
    fn blockquote_line_emits_green_fg() {
        let mut line: Line<'static> = Line::from(vec!["> ".into(), "Hello world".into()]);
        line = line.style(Color::Green);
        let spans = merged_line_spans(&line);
        assert!(spans.iter().all(|span| span.style.fg == Some(Color::Green)));
    }

    #[test]
    fn blockquote_wrap_preserves_color_on_all_wrapped_lines() {
        let mut line: Line<'static> = Line::from(vec![
            "> ".into(),
            "This is a long quoted line that should wrap".into(),
        ]);
        line = line.style(Color::Green);
        let wrapped =
            crate::wrapping::word_wrap_lines(vec![line], crate::wrapping::RtOptions::new(20));

        assert!(
            wrapped.len() >= 2,
            "expected wrapped output to span >=2 rows"
        );
        for wrapped_line in wrapped {
            let spans = merged_line_spans(&wrapped_line);
            assert!(
                spans.iter().all(|span| span.style.fg == Some(Color::Green)),
                "expected wrapped line to preserve green style, got {spans:?}",
            );
        }
    }

    #[test]
    fn colored_prefix_then_plain_text_resets_color() {
        let line: Line<'static> = Line::from(vec![
            Span::styled("1. ", ratatui::style::Style::default().fg(Color::LightBlue)),
            Span::raw("Hello world"),
        ]);
        let spans = merged_line_spans(&line);
        assert_eq!(spans[0].style.fg, Some(Color::LightBlue));
        assert_eq!(spans[1].style.fg, None);
    }

    #[test]
    fn long_agent_link_continuation_rows_have_no_padding() {
        let width: u16 = 40;
        let height: u16 = 12;
        let backend = VT100Backend::new(width, height);
        let mut term = crate::custom_terminal::Terminal::with_options(backend).expect("terminal");
        term.set_viewport_area(Rect::new(0, height - 1, width, 1));

        let url = "https://checkout.stripe.com/c/pay/cs_live_abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789#fidkdWxOYHwnPyd1blpxYHZxWjA0V1dH%2FJ2FgY2Rw";
        let markdown = format!("[Open Stripe Checkout]({url})");
        let cell = AgentMessageCell::new(render_markdown_text(&markdown).lines, true);
        let display_lines = cell.display_lines(width);

        assert!(
            display_lines.iter().any(|line| line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .contains(url)),
            "agent history must keep the complete Stripe URL in one logical line: {display_lines:?}",
        );

        insert_history_lines(&mut term, display_lines).expect("insert link history");

        let rows = term
            .backend()
            .vt100()
            .screen()
            .rows(0, width)
            .map(|row| row.trim_end().to_string())
            .collect::<Vec<_>>();
        let first_url_row = rows
            .iter()
            .position(|row| row.contains("https://"))
            .unwrap_or_else(|| panic!("expected URL start in rows: {rows:?}"));
        let last_url_row = rows
            .iter()
            .position(|row| row.contains("%2FJ2FgY2Rw)"))
            .unwrap_or_else(|| panic!("expected URL tail in rows: {rows:?}"));

        assert!(
            last_url_row > first_url_row,
            "expected URL to wrap: {rows:?}"
        );
        assert!(
            rows[first_url_row + 1..=last_url_row]
                .iter()
                .all(|row| !row.starts_with(' ')),
            "terminal-wrapped URL continuation rows must start at column zero: {rows:?}",
        );
        assert!(
            (first_url_row..last_url_row).all(|row| term
                .backend()
                .vt100()
                .screen()
                .row_wrapped(row as u16)),
            "Stripe URL must use terminal soft-wraps rather than inserted newlines: {rows:?}",
        );
        insta::assert_snapshot!("long_agent_stripe_link_soft_wrap", rows.join("\n"));
    }

    #[test]
    fn direct_write_keeps_long_agent_link_soft_wrapped() {
        let width: u16 = 40;
        let height: u16 = 12;
        let backend = VT100Backend::new(width, height);
        let mut term = crate::custom_terminal::Terminal::with_options(backend).expect("terminal");
        term.set_viewport_area(Rect::new(0, 8, width, 4));

        let url = "https://checkout.stripe.com/c/pay/cs_live_abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789#fidkdWxOYHwnPyd1blpxYHZxWjA0V1dH%2FJ2FgY2Rw";
        let markdown = format!("[Open Stripe Checkout]({url})");
        let cell = AgentMessageCell::new(render_markdown_text(&markdown).lines, true);
        let mut lines = cell.display_lines(width);

        write_pending_lines_directly(&mut term, &mut lines, 8).expect("write pending link");

        let screen = term.backend().vt100().screen();
        let rows = screen
            .rows(0, width)
            .map(|row| row.trim_end().to_string())
            .collect::<Vec<_>>();
        let first_url_row = rows
            .iter()
            .position(|row| row.contains("https://"))
            .unwrap_or_else(|| panic!("expected URL start in rows: {rows:?}"));
        let last_url_row = rows
            .iter()
            .position(|row| row.contains("%2FJ2FgY2Rw)"))
            .unwrap_or_else(|| panic!("expected URL tail in rows: {rows:?}"));

        assert!(
            last_url_row > first_url_row,
            "expected URL to wrap: {rows:?}"
        );
        assert!(
            (first_url_row..last_url_row).all(|row| screen.row_wrapped(row as u16)),
            "direct history writes must preserve terminal soft-wraps: {rows:?}",
        );
    }

    #[test]
    fn mixed_url_history_wraps_prose_before_the_url() {
        let width: u16 = 20;
        let height: u16 = 10;
        let backend = VT100Backend::new(width, height);
        let mut term = crate::custom_terminal::Terminal::with_options(backend).expect("terminal");
        term.set_viewport_area(Rect::new(0, height - 1, width, 1));

        let line = Line::from(
            "see phrase before https://example.test/abcdefghijklmnopqrstuvwxyz tail words",
        );
        insert_history_lines(&mut term, vec![line]).expect("insert mixed URL history");

        let rows = term
            .backend()
            .vt100()
            .screen()
            .rows(0, width)
            .map(|row| row.trim_end().to_string())
            .collect::<Vec<_>>();

        assert!(
            rows.iter().any(|row| row == "see phrase before"),
            "prose should wrap at a word boundary before the URL: {rows:?}",
        );
        assert!(
            rows.iter().any(|row| row.starts_with("https://")),
            "URL should begin as an intact token: {rows:?}",
        );
    }

    #[test]
    fn deep_nested_mixed_list_third_level_marker_is_colored() {
        let md = "1. First\n   - Second level\n     1. Third level (ordered)\n        - Fourth level (bullet)\n          - Fifth level to test indent consistency\n";
        let text = render_markdown_text(md);
        let lines: Vec<Line<'static>> = text.lines.clone();
        let needle = "Third level (ordered)";
        let line = lines
            .into_iter()
            .find(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
                    .contains(needle)
            })
            .expect("expected ordered nested list line");
        let spans = merged_line_spans(&line);
        assert!(
            spans
                .iter()
                .any(|span| span.style.fg == Some(Color::LightBlue))
        );
        assert!(
            spans
                .iter()
                .any(|span| span.content.contains("Third level (ordered)")
                    && span.style.fg.is_none())
        );
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

    /// When exactly one row exists above the viewport (area.top() == 1), the
    /// insertion scroll region degenerates to a single row. DECSTBM requires
    /// the bottom margin to exceed the top margin, so terminals (and the vt100
    /// parser used here) ignore the degenerate region and fall back to a
    /// full-screen scroll region — every inserted line then scrolls the
    /// viewport itself. The viewport content must survive insertion.
    #[test]
    fn single_row_above_viewport_does_not_corrupt_display() {
        let width: u16 = 40;
        let height: u16 = 10;
        let backend = VT100Backend::new(width, height);
        let mut term = crate::custom_terminal::Terminal::with_options(backend).expect("terminal");

        // Viewport occupies rows 1..10, leaving exactly one row above.
        let viewport = Rect::new(0, 1, width, height - 1);
        term.set_viewport_area(viewport);

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

        let before: Vec<String> = term.backend().vt100().screen().rows(0, width).collect();
        assert!(
            before[1].contains("Viewport row 0"),
            "draw did not reach the vt100 screen; test would be vacuous, got: {before:?}"
        );

        // Insert a batch larger than the single row above the viewport.
        let lines: Vec<Line> = (0..8).map(|i| Line::from(format!("History {i}"))).collect();
        insert_history_lines(&mut term, lines).expect("insert");
        Backend::flush(term.backend_mut()).expect("flush");

        let after: Vec<String> = term.backend().vt100().screen().rows(0, width).collect();

        // Insertion is a no-op here (no usable room), so the whole screen —
        // including the single free row above the viewport — must be unchanged.
        pretty_assertions::assert_eq!(
            before,
            after,
            "screen content was corrupted by insert_history_lines when area.top()==1"
        );
    }

    /// With only one row above the viewport, a multi-line batch cannot be
    /// inserted through a scroll region. insert_history_lines must report
    /// that the lines were NOT inserted so callers retain them.
    #[test]
    fn single_row_above_viewport_returns_false() {
        let width: u16 = 40;
        let height: u16 = 10;
        let backend = VT100Backend::new(width, height);
        let mut term = crate::custom_terminal::Terminal::with_options(backend).expect("terminal");

        let viewport = Rect::new(0, 1, width, height - 1);
        term.set_viewport_area(viewport);

        let lines: Vec<Line> = (0..8).map(|i| Line::from(format!("History {i}"))).collect();
        let inserted = insert_history_lines(&mut term, lines).expect("insert");
        assert!(
            !inserted,
            "insert_history_lines should return false when area.top() == 1"
        );
    }

    /// A viewport at y == 0 that leaves room below is first scrolled down to
    /// make space. When that scroll amount is 1, the viewport lands at y == 1
    /// and the subsequent insertion hits the same degenerate scroll region.
    /// The viewport content must survive.
    #[test]
    fn viewport_scrolled_down_to_row_one_does_not_corrupt_display() {
        let width: u16 = 40;
        let height: u16 = 10;
        let backend = VT100Backend::new(width, height);
        let mut term = crate::custom_terminal::Terminal::with_options(backend).expect("terminal");

        // Viewport at the very top with room below; a 1-line insert scrolls
        // it down by exactly one row.
        let viewport_h: u16 = 5;
        let viewport = Rect::new(0, 0, width, viewport_h);
        term.set_viewport_area(viewport);

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

        let inserted =
            insert_history_lines(&mut term, vec![Line::from("History entry")]).expect("insert");
        Backend::flush(term.backend_mut()).expect("flush");

        // With the viewport scrolled down to row 1, there is still no usable
        // scroll region, so the line must be reported as not inserted.
        assert!(
            !inserted,
            "insert_history_lines should return false when the scroll-down lands the viewport at y==1"
        );

        let rows: Vec<String> = term.backend().vt100().screen().rows(0, width).collect();

        // The 1-line insert scrolls the viewport down by exactly one row, and
        // the early return must persist that move on the terminal.
        let vp_start = rows
            .iter()
            .position(|r| r.contains("Viewport row 0"))
            .unwrap_or_else(|| panic!("viewport content lost after insertion, rows: {rows:?}"));
        pretty_assertions::assert_eq!(
            vp_start,
            1,
            "viewport should have been scrolled down exactly one row"
        );
        pretty_assertions::assert_eq!(
            term.viewport_area.y,
            1,
            "moved viewport position should be persisted on early return"
        );
        for i in 0..viewport_h as usize {
            let row_text = &rows[vp_start + i];
            assert!(
                row_text.contains(&format!("Viewport row {i}")),
                "viewport row {i} corrupted after scroll-down insertion, got: {row_text:?}"
            );
        }
    }

    /// The deferral at y == 1 is self-healing: retrying the same insert (as
    /// Tui::draw does with retained pending lines) scrolls the viewport
    /// further down, succeeds, and places the line above the viewport.
    #[test]
    fn insertion_retry_succeeds_after_deferral_at_row_one() {
        let width: u16 = 40;
        let height: u16 = 10;
        let backend = VT100Backend::new(width, height);
        let mut term = crate::custom_terminal::Terminal::with_options(backend).expect("terminal");

        let viewport_h: u16 = 5;
        term.set_viewport_area(Rect::new(0, 0, width, viewport_h));

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

        // First attempt: the viewport scrolls down to y=1 and the insert is
        // deferred (degenerate region).
        let first =
            insert_history_lines(&mut term, vec![Line::from("History entry")]).expect("insert");
        assert!(!first, "first insert should be deferred at y==1");

        // Retry with the same line, as Tui::draw does with retained pending
        // lines. The viewport scrolls to y=2, opening a valid region.
        let second =
            insert_history_lines(&mut term, vec![Line::from("History entry")]).expect("insert");
        Backend::flush(term.backend_mut()).expect("flush");
        assert!(second, "retried insert should succeed once room exists");

        let rows: Vec<String> = term.backend().vt100().screen().rows(0, width).collect();
        let history_row = rows
            .iter()
            .position(|r| r.contains("History entry"))
            .unwrap_or_else(|| panic!("history line should appear after retry, rows: {rows:?}"));
        let vp_start = rows
            .iter()
            .position(|r| r.contains("Viewport row 0"))
            .unwrap_or_else(|| panic!("viewport content lost after retry, rows: {rows:?}"));
        assert!(
            history_row < vp_start,
            "history (row {history_row}) should sit above the viewport (row {vp_start})"
        );
        for i in 0..viewport_h as usize {
            let row_text = &rows[vp_start + i];
            assert!(
                row_text.contains(&format!("Viewport row {i}")),
                "viewport row {i} corrupted after retry, got: {row_text:?}"
            );
        }
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

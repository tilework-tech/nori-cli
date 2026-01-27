use std::fmt;
use std::io;
use std::io::Write;

use crate::wrapping::word_wrap_lines_borrowed;
use crossterm::Command;
use crossterm::cursor::MoveTo;
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

/// Insert `lines` above the viewport using the terminal's backend writer
/// (avoids direct stdout references).
pub fn insert_history_lines<B>(
    terminal: &mut crate::custom_terminal::Terminal<B>,
    lines: Vec<Line>,
) -> io::Result<()>
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
            SetForegroundColor(
                line.style
                    .fg
                    .map(std::convert::Into::into)
                    .unwrap_or(CColor::Reset)
            ),
            SetBackgroundColor(
                line.style
                    .bg
                    .map(std::convert::Into::into)
                    .unwrap_or(CColor::Reset)
            )
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
        write_spans(writer, merged_spans.iter(), line.style)?;
    }

    queue!(writer, ResetScrollRegion)?;

    // Restore the cursor position to where it was before we started.
    queue!(writer, MoveTo(last_cursor_pos.x, last_cursor_pos.y))?;

    let _ = writer;
    if should_update_area {
        terminal.set_viewport_area(area);
    }

    Ok(())
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

fn write_spans<'a, I>(
    mut writer: &mut impl Write,
    content: I,
    line_style: ratatui::style::Style,
) -> io::Result<()>
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
        let next_fg = span.style.fg.or(line_style.fg).unwrap_or(Color::Reset);
        let next_bg = span.style.bg.or(line_style.bg).unwrap_or(Color::Reset);
        if next_fg != fg || next_bg != bg {
            queue!(
                writer,
                SetForegroundColor(next_fg.into()),
                SetBackgroundColor(next_bg.into())
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
    use crate::wrapping::word_wrap_lines_borrowed;

    use ratatui::style::Color;

    #[test]
    fn writes_bold_then_regular_spans() {
        use ratatui::style::Stylize;

        let spans = ["A".bold(), "B".into()];

        let mut actual: Vec<u8> = Vec::new();
        write_spans(&mut actual, spans.iter(), ratatui::style::Style::default()).unwrap();

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
        let mut line: Line<'static> = Line::from(vec!["> ".into(), "Hello world".into()]);
        line = line.style(Color::Green);

        let wrapped = word_wrap_lines_borrowed([&line], 40);
        let mut saw_colored = false;
        for line in wrapped {
            for span in &line.spans {
                if span.content.as_ref().trim().is_empty() {
                    continue;
                }
                if span.style.fg == Some(Color::Green) {
                    saw_colored = true;
                    break;
                }
            }
        }

        assert!(saw_colored, "expected at least one green span");
    }

    #[test]
    fn vt100_blockquote_wrap_preserves_color_on_all_wrapped_lines() {
        let mut line: Line<'static> = Line::from(vec![
            "> ".into(),
            "This is a long quoted line that should wrap".into(),
        ]);
        line = line.style(Color::Green);

        let wrapped = word_wrap_lines_borrowed([&line], 20);
        assert!(
            wrapped.len() >= 2,
            "expected wrapped output to span >=2 rows, got {}",
            wrapped.len()
        );
        for line in wrapped {
            for span in &line.spans {
                if span.content.as_ref().trim().is_empty() {
                    continue;
                }
                assert_eq!(
                    span.style.fg,
                    Some(Color::Green),
                    "expected wrapped span to remain green"
                );
            }
        }
    }

    #[test]
    fn vt100_colored_prefix_then_plain_text_resets_color() {
        let line: Line<'static> = Line::from(vec![
            Span::styled("1. ", ratatui::style::Style::default().fg(Color::LightBlue)),
            Span::raw("Hello world"),
        ]);

        assert_eq!(line.spans[0].style.fg, Some(Color::LightBlue));
        assert_eq!(line.spans[1].style.fg, None);
    }

    #[test]
    fn vt100_deep_nested_mixed_list_third_level_marker_is_colored() {
        // Markdown with five levels (ordered → unordered → ordered → unordered → unordered).
        let md = "1. First\n   - Second level\n     1. Third level (ordered)\n        - Fourth level (bullet)\n          - Fifth level to test indent consistency\n";
        let text = render_markdown_text(md);
        let lines: Vec<Line<'static>> = text.lines.clone();

        let needle = "1. Third level (ordered)";
        let row = lines
            .iter()
            .find(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
                    .contains(needle)
            })
            .unwrap_or_else(|| panic!("expected to find line containing {needle:?}"));

        let marker_span = row
            .spans
            .iter()
            .find(|span| span.content.as_ref().contains("1."))
            .expect("expected marker span");
        assert!(marker_span.style.fg.is_some());

        let content_span = row
            .spans
            .iter()
            .find(|span| span.content.as_ref().contains("Third level"))
            .expect("expected content span");
        assert_eq!(content_span.style.fg, None);
    }
}

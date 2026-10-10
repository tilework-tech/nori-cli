//! A fixed-resolution capacity map. Values and styling belong to the caller.

use ratatui::text::Line;
use ratatui::text::Span;

use crate::Theme;

/// A 200-cell capacity map. The last occupied cell marks the current boundary
/// with a divided square in the theme's pointer color; other occupied cells
/// use crosshatched squares in normal foreground. Outlined and diagonal-fill
/// squares distinguish free and unknown capacity without relying on color.
/// Missing measurements render unknown cells, never an apparently empty map.
#[derive(Debug, Clone, Copy)]
pub struct BlockMeter {
    pub used: Option<i64>,
    pub capacity: Option<i64>,
}

/// An observed span of occupied capacity, in tokens.
#[derive(Debug, Clone, Copy)]
pub struct BlockMeterSegment {
    pub tokens: i64,
    pub current: bool,
}

impl BlockMeter {
    pub const ALTERNATE_GLYPH: &str = "▤";

    pub const USED_GLYPH: &str = "▦";
    pub const BOUNDARY_GLYPH: &str = "◩";
    pub const FREE_GLYPH: &str = "□";
    pub const UNKNOWN_GLYPH: &str = "▨";

    pub const SEGMENT_GLYPHS: [&str; 2] = [Self::USED_GLYPH, Self::ALTERNATE_GLYPH];

    pub fn lines(self, width: u16, theme: Theme) -> Vec<Line<'static>> {
        self.segmented_lines(width, theme, &[])
    }

    /// Render chronological observed spans. Boundaries are rounded cumulatively,
    /// so several sub-cell spans never inflate the occupied capacity.
    pub fn segmented_lines(
        self,
        width: u16,
        theme: Theme,
        segments: &[BlockMeterSegment],
    ) -> Vec<Line<'static>> {
        if width == 0 {
            return Vec::new();
        }
        let columns = i64::from(width.div_ceil(2)).min(20);
        let occupied = self
            .used
            .filter(|used| *used >= 0)
            .zip(self.capacity.filter(|capacity| *capacity > 0))
            .map(|(used, capacity)| {
                ((used.min(capacity) as f64 / capacity as f64) * 200.0).ceil() as i64
            });
        let mut total = 0_i64;
        let boundaries: Vec<_> = segments
            .iter()
            .enumerate()
            .map(|(index, segment)| {
                total = total.saturating_add(segment.tokens.max(0));
                let end = self
                    .capacity
                    .filter(|capacity| *capacity > 0)
                    .map_or(0, |capacity| {
                        ((total.min(capacity) as f64 / capacity as f64) * 200.0).ceil() as i64
                    });
                (index, end, segment.current)
            })
            .collect();
        (0..200)
            .collect::<Vec<i64>>()
            .chunks(columns as usize)
            .map(|cells| {
                let mut spans = Vec::new();
                for (column, cell) in cells.iter().enumerate() {
                    if column > 0 {
                        spans.push(Span::raw(" "));
                    }
                    let (glyph, style) = match occupied {
                        None => (Self::UNKNOWN_GLYPH, theme.muted),
                        Some(count) if *cell == count - 1 => (Self::BOUNDARY_GLYPH, theme.pointer),
                        Some(count) if *cell < count => boundaries
                            .iter()
                            .find(|(_, end, _)| *cell < *end)
                            .map(|(index, _, current)| {
                                (
                                    Self::SEGMENT_GLYPHS[index % Self::SEGMENT_GLYPHS.len()],
                                    if *current { theme.pointer } else { theme.text },
                                )
                            })
                            .unwrap_or((Self::USED_GLYPH, theme.text)),
                        Some(_) => (Self::FREE_GLYPH, theme.muted),
                    };
                    spans.push(Span::styled(glyph, style));
                }
                Line::from(spans)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn segments_alternate_glyphs_and_highlight_the_current_turn() {
        let theme = Theme::default();
        let lines = BlockMeter {
            used: Some(60),
            capacity: Some(200),
        }
        .segmented_lines(
            39,
            theme,
            &[
                BlockMeterSegment {
                    tokens: 20,
                    current: false,
                },
                BlockMeterSegment {
                    tokens: 25,
                    current: false,
                },
                BlockMeterSegment {
                    tokens: 15,
                    current: true,
                },
            ],
        );
        let mut buffer = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 39, 10));
        ratatui::widgets::Widget::render(
            ratatui::widgets::Paragraph::new(lines),
            buffer.area,
            &mut buffer,
        );
        assert_eq!(buffer[(0, 0)].symbol(), BlockMeter::USED_GLYPH);
        assert_eq!(buffer[(0, 1)].symbol(), BlockMeter::ALTERNATE_GLYPH);
        assert_eq!(buffer[(10, 2)].symbol(), BlockMeter::USED_GLYPH);
        assert_eq!(buffer[(10, 2)].fg, theme.pointer.fg.unwrap());
        assert_eq!(buffer[(38, 2)].symbol(), BlockMeter::BOUNDARY_GLYPH);
        assert_eq!(buffer[(0, 3)].symbol(), BlockMeter::FREE_GLYPH);
        let highlighted = (0..10)
            .flat_map(|y| (0..20).map(move |x| (x * 2, y)))
            .filter(|position| buffer[*position].fg == theme.pointer.fg.unwrap())
            .count();
        assert_eq!(highlighted, 15);
    }

    #[test]
    fn an_even_current_segment_keeps_its_alternating_glyph() {
        let theme = Theme::default();
        let lines = BlockMeter {
            used: Some(40),
            capacity: Some(200),
        }
        .segmented_lines(
            39,
            theme,
            &[
                BlockMeterSegment {
                    tokens: 20,
                    current: false,
                },
                BlockMeterSegment {
                    tokens: 20,
                    current: true,
                },
            ],
        );
        let mut buffer = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 39, 10));
        ratatui::widgets::Widget::render(
            ratatui::widgets::Paragraph::new(lines),
            buffer.area,
            &mut buffer,
        );
        assert_eq!(buffer[(0, 1)].symbol(), BlockMeter::ALTERNATE_GLYPH);
        assert_eq!(buffer[(0, 1)].fg, theme.pointer.fg.unwrap());
        assert_eq!(buffer[(38, 1)].symbol(), BlockMeter::BOUNDARY_GLYPH);
    }

    #[test]
    fn segment_rounding_preserves_total_occupancy_and_unknown_capacity() {
        let segments = [BlockMeterSegment {
            tokens: 1,
            current: false,
        }; 3];
        for width in 0..40 {
            let lines = BlockMeter {
                used: Some(3),
                capacity: Some(1000),
            }
            .segmented_lines(width, Theme::default(), &segments);
            assert!(lines.iter().all(|line| line.width() <= usize::from(width)));
            if width > 0 {
                let text = lines.iter().map(ToString::to_string).collect::<String>();
                assert_eq!(text.matches('□').count(), 199);
            }
        }
        assert_eq!(
            BlockMeter {
                used: None,
                capacity: Some(200)
            }
            .segmented_lines(39, Theme::default(), &segments),
            BlockMeter {
                used: None,
                capacity: Some(200)
            }
            .lines(39, Theme::default())
        );
    }

    #[test]
    fn capacity_map_snapshots_and_accent() {
        let meter = BlockMeter {
            used: Some(340_412),
            capacity: Some(1_000_000),
        };
        let theme = Theme::default();
        let lines = meter.lines(39, theme);
        insta::assert_snapshot!(
            lines
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        );
        assert_eq!(
            lines
                .iter()
                .flat_map(|line| &line.spans)
                .filter(|span| span.style == theme.pointer)
                .count(),
            1
        );
        assert_eq!(
            lines
                .iter()
                .flat_map(|line| &line.spans)
                .filter(|span| matches!(span.content.as_ref(), "▦" | "◩"))
                .count(),
            69
        );
        let mut buffer = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 39, 10));
        ratatui::widgets::Widget::render(
            ratatui::widgets::Paragraph::new(lines),
            buffer.area,
            &mut buffer,
        );
        assert_eq!(buffer[(16, 3)].fg, ratatui::style::Color::Green);
        assert_eq!(buffer[(16, 3)].symbol(), "◩");
        assert_eq!(buffer[(18, 3)].fg, ratatui::style::Color::DarkGray);
        insta::assert_snapshot!(
            "narrow_capacity_map",
            meter
                .lines(19, theme)
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    #[test]
    fn missing_zero_and_overflow_are_distinct() {
        for (used, capacity, filled, unknown) in [
            (None, Some(100), 0, 200),
            (Some(10), None, 0, 200),
            (Some(-1), Some(100), 0, 200),
            (Some(10), Some(0), 0, 200),
            (Some(0), Some(100), 0, 0),
            (Some(101), Some(100), 200, 0),
            (Some(i64::MAX), Some(i64::MAX), 200, 0),
        ] {
            let text = BlockMeter { used, capacity }
                .lines(39, Theme::default())
                .iter()
                .map(ToString::to_string)
                .collect::<String>();
            assert_eq!(
                (text.matches(['▦', '◩']).count(), text.matches('▨').count()),
                (filled, unknown)
            );
        }
        for width in 0..40 {
            assert!(
                BlockMeter {
                    used: Some(1),
                    capacity: Some(2)
                }
                .lines(width, Theme::default())
                .iter()
                .all(|line| line.width() <= usize::from(width))
            );
        }
    }
}

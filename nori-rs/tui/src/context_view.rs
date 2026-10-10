//! Responsive, unboxed `/context` snapshot. Locally discovered files are an
//! inventory, not a claim about the agent's current post-compaction prompt.

use nori_tui_components::BlockMeter;
use nori_tui_components::Theme;
use ratatui::text::Line;
use ratatui::text::Span;

use crate::history_cell::HistoryCell;
use crate::nori::token_count::TokenCount;
use crate::nori::token_count::format_token_count;
use crate::wrapping::RtOptions;
use crate::wrapping::word_wrap_lines;

#[derive(Debug, Default)]
pub(crate) struct ContextView {
    pub(crate) used: Option<i64>,
    pub(crate) capacity: Option<i64>,
    pub(crate) source: String,
    pub(crate) agent: String,
    pub(crate) turns: i64,
    pub(crate) instructions: Vec<(String, Option<i64>)>,
    pub(crate) skills: Vec<String>,
    pub(crate) subagents: Vec<String>,
    pub(crate) remote: bool,
}

impl HistoryCell for ContextView {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        if width == 0 {
            return Vec::new();
        }
        let theme = crate::style::component_theme();
        let inner = width.saturating_sub(4).max(1);
        let mut lines = vec![
            Line::styled("Context", theme.title),
            Line::styled(
                format!(
                    "{} · {} user {} · /context",
                    self.agent,
                    self.turns,
                    if self.turns == 1 { "turn" } else { "turns" }
                ),
                theme.muted,
            ),
            Line::default(),
        ];
        let used = self.used.filter(|tokens| *tokens >= 0);
        let capacity = self.capacity.filter(|tokens| *tokens > 0);
        let measured = used.zip(capacity);
        let mut summary = vec![Line::from(vec![
            Span::styled(
                used.map(number).unwrap_or_else(|| "Not reported".into()),
                theme.title,
            ),
            Span::styled(
                format!(
                    " / {} tokens",
                    capacity.map(number).unwrap_or_else(|| "unknown".into())
                ),
                theme.muted,
            ),
        ])];
        if let Some((used, capacity)) = measured {
            summary.push(Line::styled(
                format!("{:.1}% used", used as f64 / capacity as f64 * 100.0),
                theme.text,
            ));
            summary.push(Line::default());
            summary.push(row("Used", number(used), theme));
            summary.push(row(
                "Free",
                number(capacity.saturating_sub(used).max(0)),
                theme,
            ));
        } else {
            summary.push(Line::styled("Waiting for context usage", theme.muted));
        }
        if inner >= 39 {
            summary.push(Line::default());
            if measured.is_some() {
                summary.push(Line::from(vec![
                    Span::styled(BlockMeter::BOUNDARY_GLYPH, theme.pointer),
                    Span::styled(" Current fill boundary", theme.muted),
                ]));
                summary.push(Line::styled(
                    format!("{} Free capacity", BlockMeter::FREE_GLYPH),
                    theme.muted,
                ));
            } else {
                summary.push(Line::styled(
                    format!("{} Usage not reported", BlockMeter::UNKNOWN_GLYPH),
                    theme.muted,
                ));
            }
            summary.push(Line::styled("Each cell = 0.5% of the window", theme.muted));
        }

        // A map remains useful at ordinary terminal sizes. On very narrow
        // terminals use the same numerical summary rather than a 200-row map.
        if inner >= 39 {
            let map = BlockMeter { used, capacity }.lines(39, theme);
            if inner >= 82 {
                lines.extend(columns(&[(map, 39), (summary, usize::from(inner) - 43)]));
            } else {
                lines.extend(summary);
                lines.push(Line::default());
                lines.extend(map);
            }
        } else {
            lines.extend(summary);
        }
        lines.push(Line::default());
        if used.is_some() {
            lines.push(Line::styled(self.source.clone(), theme.muted));
        }
        lines.push(Line::styled("Usage details", theme.title));
        lines.push(Line::styled(
            "Category split and compaction threshold are not reported by the agent.",
            theme.muted,
        ));
        lines.push(Line::default());
        let wide = inner >= 90;
        let inventory_width = if wide {
            usize::from(inner) / 2
        } else {
            usize::from(inner)
        };
        let mut inventory = vec![Line::styled(
            "Local instructions · estimated tokens",
            theme.title,
        )];
        if self.remote {
            inventory.push(Line::styled(
                "Remote instruction files are not available locally.",
                theme.muted,
            ));
        } else if self.instructions.is_empty() {
            inventory.push(Line::styled(
                "No local instruction files found for this agent.",
                theme.muted,
            ));
        } else {
            for (path, tokens) in &self.instructions {
                let value = tokens
                    .map(|tokens| format!("~{}", number(tokens)))
                    .unwrap_or_else(|| "Not readable".into());
                let label_width = inventory_width.saturating_sub(value.len() + 2).max(1);
                let label = crate::text_formatting::truncate_text(path, label_width);
                inventory.push(Line::from(vec![
                    Span::styled(format!("{label:<label_width$}  "), theme.text),
                    Span::styled(value, theme.muted),
                ]));
            }
        }
        let mut groups = vec![(inventory, inventory_width)];
        let activity_width = if wide {
            (usize::from(inner) - inventory_width - 8) / 2
        } else {
            usize::from(inner)
        };
        for (title, entries) in [
            ("Skills observed", &self.skills),
            ("Subagents observed", &self.subagents),
        ] {
            let mut group = vec![Line::styled(title, theme.title)];
            if entries.is_empty() {
                group.push(Line::styled("None observed", theme.muted));
            } else {
                group.extend(
                    entries
                        .iter()
                        .map(|entry| Line::styled(entry.clone(), theme.text)),
                );
            }
            groups.push((group, activity_width));
        }
        if wide {
            lines.extend(columns(&groups));
        } else {
            for (index, (group, _)) in groups.into_iter().enumerate() {
                if index > 0 {
                    lines.push(Line::default());
                }
                lines.extend(group);
            }
        }
        if !self.instructions.is_empty() {
            lines.push(Line::default());
            lines.push(Line::styled(
                "Local file sizes; inclusion in the current context is not verified.",
                theme.muted,
            ));
        }
        let wrapped = word_wrap_lines(&lines, RtOptions::new(usize::from(inner)));
        let inset = " ".repeat(usize::from(width.saturating_sub(1).min(2)));
        wrapped
            .into_iter()
            .map(|mut line| {
                line.spans.insert(0, Span::raw(inset.clone()));
                line
            })
            .collect()
    }
}

fn columns(groups: &[(Vec<Line<'static>>, usize)]) -> Vec<Line<'static>> {
    let wrapped: Vec<_> = groups
        .iter()
        .map(|(lines, width)| (word_wrap_lines(lines, RtOptions::new(*width)), *width))
        .collect();
    let height = wrapped
        .iter()
        .map(|(lines, _)| lines.len())
        .max()
        .unwrap_or(0);
    (0..height)
        .map(|index| {
            let mut result = Line::default();
            for (column, (lines, width)) in wrapped.iter().enumerate() {
                let line = lines.get(index).cloned().unwrap_or_default();
                let padding = width.saturating_sub(line.width()) + 4;
                result.spans.extend(line.spans);
                if column + 1 < wrapped.len() {
                    result.spans.push(Span::raw(" ".repeat(padding)));
                }
            }
            result
        })
        .collect()
}

fn number(tokens: i64) -> String {
    format_token_count(&TokenCount {
        count: tokens,
        approximate: false,
    })
    .split(' ')
    .next()
    .unwrap_or_default()
    .to_string()
}

fn row(label: &str, value: String, theme: Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<8}  "), theme.muted),
        Span::styled(value, theme.text),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn specimen() -> ContextView {
        ContextView {
            used: Some(340_412),
            capacity: Some(1_000_000),
            source: "Agent-reported usage".into(),
            agent: "Claude Code".into(),
            turns: 6,
            instructions: vec![
                ("~/AGENTS.md".into(), Some(4_112)),
                ("cli/AGENTS.md".into(), Some(2_406)),
            ],
            skills: vec!["nori-design".into(), "nori-theming".into()],
            subagents: vec!["code-review".into()],
            remote: false,
        }
    }

    #[test]
    fn context_snapshots() {
        for width in [100, 60, 32] {
            let lines = specimen().display_lines(width);
            insta::assert_snapshot!(
                format!("context_{width}"),
                lines
                    .iter()
                    .map(|line| line.to_string().trim_end().to_string())
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            assert!(lines.iter().all(|line| line.width() <= usize::from(width)));
        }
        insta::assert_snapshot!(
            "context_unknown",
            ContextView {
                agent: "ElizACP".into(),
                remote: true,
                ..Default::default()
            }
            .display_lines(90)
            .iter()
            .map(|line| line.to_string().trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n")
        );
    }
}

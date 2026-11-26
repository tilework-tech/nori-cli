//! Responsive, unboxed `/context` snapshot. Locally discovered files are an
//! inventory, not a claim about the agent's current post-compaction prompt.

use nori_tui_components::BlockMeter;
use nori_tui_components::BlockMeterSegment;
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
    pub(crate) timeline: nori_harness::context_timeline::ContextTimeline,
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
        let render_started = std::time::Instant::now();
        if width == 0 {
            return Vec::new();
        }
        let theme = crate::style::component_theme();
        let inner = width.saturating_sub(4).max(1);
        let recorded_turns = self.timeline.turns.len() as i64;
        let turns = if self.timeline.turns.is_empty() || self.timeline.incomplete_history {
            self.turns.max(recorded_turns)
        } else {
            recorded_turns
        };
        let checkpoint_count = if self.timeline.incomplete_history {
            format!(" · {recorded_turns} checkpointed")
        } else {
            String::new()
        };
        let current_turn = self.timeline.turns.last();
        let mut lines = vec![
            Line::styled("Context", theme.title),
            Line::styled(
                format!(
                    "{} · {} user {}{checkpoint_count} · /context",
                    self.agent,
                    turns,
                    if turns == 1 { "turn" } else { "turns" }
                ),
                theme.muted,
            ),
            Line::default(),
        ];
        let observation = self.timeline.current.as_ref();
        let allow_fallback = self.timeline.turns.is_empty()
            && self.timeline.segments.is_empty()
            && self.timeline.epoch == 0;
        let used = observation
            .map(|observation| bounded_tokens(observation.usage.used))
            .or(self.used.filter(|_| allow_fallback))
            .filter(|tokens| *tokens >= 0);
        let capacity = observation
            .map(|observation| bounded_tokens(observation.usage.size))
            .or(self.capacity.filter(|_| allow_fallback))
            .filter(|tokens| *tokens > 0);
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
        if let Some(turn) = current_turn {
            summary.push(Line::default());
            summary.push(row(
                if self.timeline.incomplete_history {
                    "Checkpoint"
                } else {
                    "Current turn"
                },
                format!(
                    "{} · {}",
                    turn.number,
                    if turn.end.is_some() {
                        "ended"
                    } else {
                        "running"
                    }
                ),
                theme,
            ));
            summary.push(row(
                "Turn tokens",
                change(self.timeline.current_turn_delta()),
                theme,
            ));
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
            let segments: Vec<_> = self
                .timeline
                .segments
                .iter()
                .map(|segment| BlockMeterSegment {
                    tokens: segment.tokens,
                    current: segment.turn.is_some()
                        && segment.turn == current_turn.map(|turn| turn.number),
                })
                .collect();
            let map = BlockMeter { used, capacity }.segmented_lines(39, theme, &segments);
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
        let mut current = vec![Span::styled("Current · ", theme.muted)];
        if self.timeline.epoch > 0 {
            current.push(Span::styled("context reset · ", theme.muted));
        }
        if self.timeline.segments.is_empty() {
            let status = if used.is_none() {
                "usage not reported".to_string()
            } else if observation.is_none() && !self.source.is_empty() {
                self.source.clone()
            } else {
                "no segments recorded".to_string()
            };
            current.push(Span::styled(status, theme.muted));
        } else {
            for (index, segment) in self.timeline.segments.iter().enumerate() {
                if index > 0 {
                    current.push(Span::styled(" + ", theme.muted));
                }
                let active =
                    segment.turn.is_some() && segment.turn == current_turn.map(|turn| turn.number);
                let label = segment
                    .turn
                    .map(|turn| format!("T{turn}"))
                    .unwrap_or_else(|| "Baseline".into());
                current.push(Span::styled(
                    format!(
                        "{} {label} {}",
                        BlockMeter::SEGMENT_GLYPHS[index % BlockMeter::SEGMENT_GLYPHS.len()],
                        number(segment.tokens)
                    ),
                    if active { theme.pointer } else { theme.text },
                ));
            }
        }
        lines.push(Line::from(current));
        lines.push(Line::default());
        lines.extend(self.checkpoint_lines(inner, theme));
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
        let rendered: Vec<_> = wrapped
            .into_iter()
            .map(|mut line| {
                line.spans.insert(0, Span::raw(inset.clone()));
                line
            })
            .collect();
        tracing::debug!(target: "nori_context_profile", stage = "context_render", width, turns = self.timeline.turns.len(), lines = rendered.len(), elapsed_us = render_started.elapsed().as_micros(), "context snapshot rendered");
        rendered
    }
}

impl ContextView {
    fn checkpoint_lines(&self, width: u16, theme: Theme) -> Vec<Line<'static>> {
        let mut lines = vec![Line::styled("Turn checkpoints", theme.title)];
        if self.timeline.incomplete_history {
            lines.push(Line::styled(
                "Incomplete history · some turn boundaries were not recorded.",
                theme.muted,
            ));
        }
        if self.timeline.turns.is_empty() {
            lines.push(Line::styled("No turn checkpoints recorded.", theme.muted));
            return lines;
        }
        if width >= 74 {
            lines.push(Line::styled(
                format!(
                    "{:<6} {:<17} {:<15} {:<11} Reading",
                    "Turn", "Start", "End", "Change"
                ),
                theme.muted,
            ));
        }
        for turn in &self.timeline.turns {
            let start = reading(turn.start.observation.as_ref());
            let end = turn
                .end
                .as_ref()
                .map(|checkpoint| reading(checkpoint.observation.as_ref()))
                .unwrap_or_else(|| "running".into());
            let delta = if turn.end.is_some() {
                turn.delta()
            } else {
                self.timeline.current_turn_delta()
            };
            let status = match &turn.end {
                None => "during turn",
                Some(end) if end.epoch != turn.start.epoch => "context reset",
                Some(end) if end.observation.is_none() => "not reported",
                Some(end)
                    if end.observation.as_ref().map(|observation| &observation.id)
                        == turn
                            .start
                            .observation
                            .as_ref()
                            .map(|observation| &observation.id) =>
                {
                    "no new reading"
                }
                Some(_) => "observed",
            };
            if width >= 74 {
                lines.push(Line::styled(
                    format!(
                        "{:<6} {start:<17} {end:<15} {:<11} {status}",
                        format!("T{}", turn.number),
                        change(delta)
                    ),
                    theme.text,
                ));
            } else {
                lines.push(Line::styled(
                    format!("T{} · {} · {status}", turn.number, change(delta)),
                    theme.text,
                ));
                lines.push(Line::styled(
                    format!("Start {start} · End {end}"),
                    theme.muted,
                ));
            }
        }
        lines.push(Line::styled(
            "Starts carry the latest prior observation.",
            theme.muted,
        ));
        lines
    }
}

fn bounded_tokens(tokens: u64) -> i64 {
    i64::try_from(tokens).unwrap_or(i64::MAX)
}

fn reading(observation: Option<&nori_harness::context_timeline::ContextObservation>) -> String {
    observation
        .map(|observation| number(bounded_tokens(observation.usage.used)))
        .unwrap_or_else(|| "unknown".into())
}

fn change(tokens: Option<i64>) -> String {
    tokens
        .map(|tokens| {
            if tokens > 0 {
                format!("+{}", number(tokens))
            } else {
                number(tokens)
            }
        })
        .unwrap_or_else(|| "unknown".into())
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
        Span::styled(format!("{label:<12}  "), theme.muted),
        Span::styled(value, theme.text),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn specimen() -> ContextView {
        ContextView {
            timeline: Default::default(),
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

    fn observation(id: &str, used: u64) -> nori_harness::context_timeline::ContextObservation {
        nori_harness::context_timeline::ContextObservation {
            id: id.into(),
            observed_at: "2026-10-10T10:00:00Z".into(),
            usage: serde_json::from_value(serde_json::json!({"used":used,"size":1_000_000}))
                .unwrap(),
        }
    }

    fn timeline_specimen() -> ContextView {
        use nori_harness::context_timeline::ContextCheckpoint;
        use nori_harness::context_timeline::ContextSegment;
        use nori_harness::context_timeline::ContextTimeline;
        use nori_harness::context_timeline::ContextTurn;
        let checkpoint = |observation| ContextCheckpoint {
            recorded_at: "2026-10-10T10:00:01Z".into(),
            observation,
            epoch: 0,
        };
        let first = observation("first", 100_000);
        let second = observation("second", 240_000);
        ContextView {
            timeline: ContextTimeline {
                epoch: 0,
                current: Some(observation("third", 340_412)),
                turns: vec![
                    ContextTurn {
                        number: 1,
                        request_id: 1.into(),
                        start: checkpoint(None),
                        end: Some(checkpoint(Some(first.clone()))),
                    },
                    ContextTurn {
                        number: 2,
                        request_id: 2.into(),
                        start: checkpoint(Some(first)),
                        end: Some(checkpoint(Some(second.clone()))),
                    },
                    ContextTurn {
                        number: 3,
                        request_id: 3.into(),
                        start: checkpoint(Some(second)),
                        end: None,
                    },
                ],
                segments: vec![
                    ContextSegment {
                        turn: None,
                        tokens: 100_000,
                    },
                    ContextSegment {
                        turn: Some(2),
                        tokens: 140_000,
                    },
                    ContextSegment {
                        turn: Some(3),
                        tokens: 100_412,
                    },
                ],
                incomplete_history: false,
            },
            ..specimen()
        }
    }

    #[test]
    fn turn_timeline_shows_observations_and_segments_in_the_main_grid() {
        let view = timeline_specimen();
        let lines = view.display_lines(100);
        let text = lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("Turn checkpoints"));
        assert!(text.contains("Start") && text.contains("End"));
        assert!(text.contains("Current turn") && text.contains("100,412"));
        assert!(text.contains("running"));
        assert!(text.contains('▦') && text.contains('▤'));
        assert!(text.contains("Current · ▦ Baseline 100,000 + ▤ T2 140,000 + ▦ T3 100,412"));
        for width in [100, 60, 32] {
            let lines = view.display_lines(width);
            assert!(lines.iter().all(|line| line.width() <= usize::from(width)));
            insta::assert_snapshot!(
                format!("context_turns_{width}"),
                lines
                    .iter()
                    .map(|line| line.to_string().trim_end().to_string())
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }
    }

    #[test]
    fn compact_middle_keeps_one_current_row_and_checkpoint_evidence() {
        let lines = timeline_specimen().display_lines(100);
        let text = lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        let current_rows = lines
            .iter()
            .filter(|line| line.to_string().contains("Current ·"))
            .count();
        assert_eq!(current_rows, 1);
        assert!(text.contains("Starts carry the latest prior observation."));
        let row = lines
            .iter()
            .position(|line| line.to_string().contains("Current ·"))
            .unwrap();
        let row_text = lines[row].to_string();
        let glyph_byte = row_text.find("▦ T3").unwrap();
        let column = row_text[..glyph_byte].chars().count() as u16;
        let mut buffer = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(
            0,
            0,
            100,
            lines.len() as u16,
        ));
        ratatui::widgets::Widget::render(
            ratatui::widgets::Paragraph::new(lines),
            buffer.area,
            &mut buffer,
        );
        assert_eq!(buffer[(column, row as u16)].symbol(), "▦");
        assert_eq!(
            buffer[(column, row as u16)].fg,
            ratatui::style::Color::Green
        );
    }

    #[test]
    fn timeline_usage_is_not_combined_with_provider_fallback() {
        let mut view = timeline_specimen();
        view.used = Some(999_999);
        view.capacity = Some(2_000_000);
        view.source = "Provider transcript fallback".into();
        let text = view
            .display_lines(100)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("340,412 / 1,000,000 tokens"));
        assert!(!text.contains("999,999") && !text.contains("Provider transcript fallback"));
    }

    #[test]
    fn incomplete_history_preserves_known_user_turn_count() {
        let mut view = timeline_specimen();
        view.turns = 20;
        view.timeline.incomplete_history = true;
        let text = view
            .display_lines(100)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("20 user turns"));
        assert!(text.contains("3 checkpointed"));
        assert!(text.contains("Checkpoint") && !text.contains("Current turn"));
    }

    #[test]
    fn a_reset_without_a_fresh_observation_does_not_revive_old_usage() {
        let mut view = timeline_specimen();
        view.timeline.epoch = 1;
        view.timeline.current = None;
        view.timeline.segments.clear();
        view.used = Some(999_999);
        view.capacity = Some(2_000_000);
        view.source = "Stale fallback".into();
        let text = view
            .display_lines(100)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("Waiting for context usage"));
        assert!(
            !text.contains("999,999")
                && !text.contains("2,000,000")
                && !text.contains("Stale fallback")
        );
        assert!(text.contains('▨'));
        assert!(text.contains("context reset"));
    }

    #[test]
    fn carried_observations_are_not_presented_as_fresh_measurements() {
        let mut view = timeline_specimen();
        view.timeline.turns[2].end = Some(view.timeline.turns[2].start.clone());
        view.timeline.incomplete_history = true;
        let text = view
            .display_lines(100)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("no new reading"));
        assert!(text.contains("Incomplete history"));
        assert!(text.contains("Starts carry the latest prior observation."));
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

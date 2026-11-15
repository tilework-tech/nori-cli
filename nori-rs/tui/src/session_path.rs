//! Lines that name a session by where it sits in its family, fitted to the
//! terminal width: the breadcrumb inside a delegated child and the line about
//! a session waiting on the user.

use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use unicode_width::UnicodeWidthStr;

/// One session on the way from a top-level session to the one on screen.
#[derive(Clone, Debug)]
pub(crate) struct PathStep {
    pub(crate) agent: String,
    pub(crate) name: String,
}

const PATH_SEPARATOR: &str = " › ";
const ELIDED: &str = "…";

/// `text` cut to at most `cells` display cells, ending in `…` when cut.
pub(crate) fn fit(text: &str, cells: usize) -> String {
    if text.width() <= cells {
        return text.to_string();
    }
    if cells == 0 {
        return String::new();
    }
    let mut kept = String::new();
    for ch in text.chars() {
        if kept.width() + ch.to_string().width() + 1 > cells {
            break;
        }
        kept.push(ch);
    }
    format!("{}…", kept.trim_end())
}

/// Share `room` cells between names, so a short name leaves its unused cells
/// to the longer ones.
fn share(names: &[&str], room: usize) -> Vec<usize> {
    let mut budgets = vec![0; names.len()];
    let mut open: Vec<usize> = (0..names.len()).collect();
    let mut left = room;
    while !open.is_empty() {
        let each = left / open.len();
        let (fits, cut): (Vec<usize>, Vec<usize>) = open
            .iter()
            .partition(|index| names[**index].width() <= each);
        if fits.is_empty() {
            for (position, index) in cut.iter().enumerate() {
                budgets[*index] = each + usize::from(position < left % open.len());
            }
            break;
        }
        for index in fits {
            budgets[index] = names[index].width();
            left -= budgets[index];
        }
        open = cut;
    }
    budgets
}

/// `root › … › current` with each session's agent, followed by `tail`
/// (status words and the key back to the parent), in at most `width` cells.
/// The top-level session and the current one always stay; sessions between
/// them give way to `…` when the names would otherwise be cut short.
pub(crate) fn breadcrumb_line(path: &[PathStep], tail: &str, width: u16) -> Line<'static> {
    let width = usize::from(width);
    let mut steps: Vec<Option<&PathStep>> = path.iter().map(Some).collect();
    let mut with_agents = true;
    let agent_label = |step: &PathStep, with_agents: bool| {
        if with_agents {
            format!("{} · ", step.agent)
        } else {
            String::new()
        }
    };
    loop {
        let fixed = tail.width()
            + PATH_SEPARATOR.width() * steps.len().saturating_sub(1)
            + steps
                .iter()
                .map(|step| {
                    step.map_or(ELIDED.width(), |step| {
                        agent_label(step, with_agents).width()
                    })
                })
                .sum::<usize>();
        let names: Vec<&str> = steps
            .iter()
            .flatten()
            .map(|step| step.name.as_str())
            .collect();
        let room = width.saturating_sub(fixed);
        let full = names.iter().map(|name| name.width()).sum::<usize>();
        // Keep every session while each name keeps a readable start.
        let elidable = steps.len() > 2 && steps[1].is_some();
        if full > room && elidable && room / names.len().max(1) < 16 {
            steps.splice(1..steps.len() - 1, [None]);
            continue;
        }
        // On a very narrow terminal the names matter more than the agents.
        if full > room && with_agents && room / names.len().max(1) < 4 {
            with_agents = false;
            continue;
        }
        let budgets = share(&names, room);
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut named = 0;
        let last = steps.len().saturating_sub(1);
        for (position, step) in steps.iter().enumerate() {
            if position > 0 {
                spans.push(PATH_SEPARATOR.dim());
            }
            match step {
                Some(step) => {
                    if with_agents {
                        spans.push(agent_label(step, true).dim());
                    }
                    let name = fit(&step.name, budgets[named]);
                    named += 1;
                    spans.push(if position == last {
                        name.bold()
                    } else {
                        name.into()
                    });
                }
                None => spans.push(ELIDED.dim()),
            }
        }
        spans.push(tail.to_string().dim());
        return clip(Line::from(spans), width);
    }
}

/// `● parent › child is waiting on you · request (+N more) · key to jump`, in
/// at most `width` cells. Names come first; the request text gets what they
/// leave, and every name keeps a share when space is short.
pub(crate) fn waiting_line(
    path: &[String],
    request: Option<&str>,
    more: usize,
    jump_key: Option<&str>,
    width: u16,
) -> Line<'static> {
    let width = usize::from(width);
    let marker = "● ";
    let label = format!(" is {}", crate::session_status::WAITING_ON_YOU_LABEL);
    let more = if more > 0 {
        format!(" (+{more} more)")
    } else {
        String::new()
    };
    let key = jump_key
        .map(|key| format!(" · {key} to jump"))
        .unwrap_or_default();
    let request_sep = " · ";
    let fixed = marker.width()
        + label.width()
        + more.width()
        + key.width()
        + PATH_SEPARATOR.width() * path.len().saturating_sub(1);
    let names: Vec<&str> = path.iter().map(String::as_str).collect();
    let room = width.saturating_sub(fixed);
    let names_full = names.iter().map(|name| name.width()).sum::<usize>();
    let request_room = request.map_or(0, |request| {
        let wanted = request.width() + request_sep.width();
        // Names come first. A cut request is shown only while it stays
        // readable and every name keeps a readable start.
        let left = room.saturating_sub(names_full);
        if left >= wanted || left >= MIN_REQUEST {
            left.min(wanted)
        } else if room.saturating_sub(MIN_REQUEST) >= MIN_NAME * names.len() {
            MIN_REQUEST.min(wanted)
        } else {
            0
        }
    });
    let budgets = share(&names, room.saturating_sub(request_room));
    let mut spans = vec![Span::styled(
        marker,
        crate::style::component_theme().warning,
    )];
    let path_text = names
        .iter()
        .zip(budgets)
        .map(|(name, budget)| fit(name, budget))
        .collect::<Vec<_>>()
        .join(PATH_SEPARATOR);
    spans.push(path_text.bold());
    spans.push(label.into());
    if let Some(request) = request
        && request_room > request_sep.width()
    {
        spans.push(
            format!(
                "{request_sep}{}",
                fit(request, request_room - request_sep.width())
            )
            .dim(),
        );
    }
    if !more.is_empty() {
        spans.push(more.dim());
    }
    if !key.is_empty() {
        spans.push(key.dim());
    }
    clip(Line::from(spans), width)
}

/// The fewest cells a cut request text gets, separator included.
const MIN_REQUEST: usize = 20;
/// The fewest cells a name keeps before a request gives way to it.
const MIN_NAME: usize = 16;

/// `line` cut to `width` cells, for terminals too narrow for its fixed parts.
fn clip(line: Line<'static>, width: usize) -> Line<'static> {
    if line.width() <= width {
        return line;
    }
    if width == 0 {
        return Line::default();
    }
    // Leave one cell for the ellipsis that marks the cut.
    let mut left = width - 1;
    let mut spans = Vec::new();
    'spans: for span in line.spans {
        let mut kept = String::new();
        for ch in span.content.chars() {
            let cells = ch.to_string().width();
            if cells > left {
                spans.push(Span::styled(kept, span.style));
                break 'spans;
            }
            left -= cells;
            kept.push(ch);
        }
        spans.push(Span::styled(kept, span.style));
    }
    spans.push(Span::raw("…"));
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    fn text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    fn step(agent: &str, name: &str) -> PathStep {
        PathStep {
            agent: agent.into(),
            name: name.into(),
        }
    }

    /// Each case rendered at its width, with a ruler, so a snapshot shows
    /// what fits.
    fn render(cases: &[(&str, u16, Line<'static>)]) -> String {
        cases
            .iter()
            .map(|(label, width, line)| {
                let text = text(line);
                assert!(
                    text.width() <= usize::from(*width),
                    "{label}: {} cells > {width}: {text}",
                    text.width()
                );
                format!("{label} @{width}\n{text}\n")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    const SHARED: &str = "mock:delegate mock-model inspect the parser";
    const KEY: &str = " · alt + up to parent";

    #[test]
    fn breadcrumbs_show_the_whole_path_and_fit_the_width() {
        let three = [
            step("claude-code", "Ship tree-row picker"),
            step("codex", "Port picker"),
            step("pi", "Write snapshot tests"),
        ];
        let shared = [
            step("mock-model", SHARED),
            step("mock-model-alt", &format!("{SHARED} again")),
        ];
        let long = [
            step(
                "mock-model",
                "A parent session with a very long title indeed",
            ),
            step(
                "mock-model-alt",
                "A child with a very long title that goes on",
            ),
            step(
                "mock-model",
                "A grandchild whose title is also very long here",
            ),
        ];
        insta::assert_snapshot!(render(&[
            ("three levels", 120, breadcrumb_line(&three, KEY, 120)),
            ("three levels", 78, breadcrumb_line(&three, KEY, 78)),
            ("shared prefix", 120, breadcrumb_line(&shared, KEY, 120)),
            ("shared prefix", 78, breadcrumb_line(&shared, KEY, 78)),
            ("long names", 118, breadcrumb_line(&long, KEY, 118)),
            ("long names", 78, breadcrumb_line(&long, KEY, 78)),
            ("long names", 40, breadcrumb_line(&long, KEY, 40)),
        ]));
    }

    #[test]
    fn a_breadcrumb_never_drops_its_root_its_current_session_or_the_key() {
        let long = [
            step(
                "mock-model",
                "A parent session with a very long title indeed",
            ),
            step(
                "mock-model-alt",
                "A child with a very long title that goes on",
            ),
            step(
                "mock-model",
                "A grandchild whose title is also very long here",
            ),
        ];
        for width in [118, 100, 78] {
            let line = text(&breadcrumb_line(&long, KEY, width));
            assert!(line.starts_with("mock-model · A "), "@{width}: {line}");
            assert!(line.contains("› mock-model · A gran"), "@{width}: {line}");
            assert!(line.ends_with(KEY), "@{width}: {line}");
        }
    }

    #[test]
    fn waiting_lines_name_parent_and_child_as_fully_as_the_width_allows() {
        let child = [
            "Ship tree-row picker".to_string(),
            "mock:request-permission:cargo test --workspace".to_string(),
        ];
        let shared = [SHARED.to_string(), format!("{SHARED} again")];
        let root = ["Spike transport".to_string()];
        let request = Some("cargo test --workspace --all-features");
        insta::assert_snapshot!(render(&[
            (
                "child",
                118,
                waiting_line(&child, request, 0, Some("alt + down"), 118)
            ),
            (
                "child",
                78,
                waiting_line(&child, request, 0, Some("alt + down"), 78)
            ),
            (
                "shared prefix",
                118,
                waiting_line(&shared, request, 1, Some("alt + down"), 118)
            ),
            (
                "shared prefix",
                78,
                waiting_line(&shared, request, 1, Some("alt + down"), 78)
            ),
            (
                "top level",
                78,
                waiting_line(&root, request, 0, Some("alt + down"), 78)
            ),
            ("no key", 78, waiting_line(&child, None, 0, None, 78)),
        ]));
    }

    #[test]
    fn a_wide_terminal_shows_names_whole() {
        let child = [
            "Ship tree-row picker".to_string(),
            "mock:request-permission:cargo test --workspace".to_string(),
        ];
        let line = text(&waiting_line(
            &child,
            Some("cargo test"),
            0,
            Some("alt + down"),
            128,
        ));
        assert!(
            line.contains("Ship tree-row picker › mock:request-permission:cargo test --workspace"),
            "{line}"
        );
        assert!(line.ends_with(" · alt + down to jump"), "{line}");
    }

    #[test]
    fn lines_never_exceed_the_terminal_width() {
        let long = [
            step(
                "mock-model",
                "A parent session with a very long title indeed",
            ),
            step(
                "mock-model-alt",
                "A child with a very long title that goes on",
            ),
            step(
                "mock-model",
                "A grandchild whose title is also very long here",
            ),
        ];
        let names = ["Ship tree-row picker".to_string(), SHARED.to_string()];
        let alone = ["Spike transport".to_string()];
        for width in 0..=120 {
            let lines = [
                breadcrumb_line(&long, KEY, width),
                breadcrumb_line(&long[..1], KEY, width),
                waiting_line(
                    &names,
                    Some("cargo test --workspace"),
                    2,
                    Some("alt + down"),
                    width,
                ),
                waiting_line(&alone, None, 0, Some("alt + down"), width),
            ];
            for line in lines {
                assert!(
                    line.width() <= usize::from(width),
                    "@{width}: {:?}",
                    text(&line)
                );
            }
        }
    }
}

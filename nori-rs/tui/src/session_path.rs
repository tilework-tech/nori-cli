//! Lines that name a session by where it sits in its family, fitted to the
//! terminal width: the breadcrumb inside a delegated child and the line about
//! a session waiting on the user.

use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use unicode_segmentation::UnicodeSegmentation;
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

/// `text` cut to at most `cells` display cells, keeping its start and end
/// around `…`, so names that share a long prefix still read apart.
pub(crate) fn fit_name(text: &str, cells: usize) -> String {
    if text.width() <= cells {
        return text.to_string();
    }
    if cells == 0 {
        return String::new();
    }
    let room = cells - 1;
    let graphemes: Vec<&str> = text.graphemes(true).collect();
    let mut head = String::new();
    for grapheme in &graphemes {
        if head.width() + grapheme.width() > room.div_ceil(2) {
            break;
        }
        head.push_str(grapheme);
    }
    let mut tail_width = 0;
    let mut tail: Vec<&str> = Vec::new();
    for grapheme in graphemes.iter().rev() {
        if head.width() + tail_width + grapheme.width() > room {
            break;
        }
        tail_width += grapheme.width();
        tail.push(grapheme);
    }
    tail.reverse();
    format!("{}…{}", head.trim_end(), tail.concat().trim_start())
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

/// The fewest cells a session other than the current one keeps before the
/// key back to the parent gives way.
const MIN_OTHER: usize = 8;
/// The fewest cells any name keeps on a terminal too narrow for the rest.
const MIN_NAME_CELLS: usize = 4;

/// `root › … › current` with each session's agent, followed by `status` and
/// `key` (the key back to the parent), in at most `width` cells.
///
/// The top-level session and the current one always stay. When space runs
/// short, sessions between them give way to `…` first, then the key, then the
/// agent labels, and the current session's name is cut last.
pub(crate) fn breadcrumb_line(
    path: &[PathStep],
    status: &str,
    key: &str,
    width: u16,
) -> Line<'static> {
    let width = usize::from(width);
    let whole: Vec<Option<&PathStep>> = path.iter().map(Some).collect();
    let elided: Vec<Option<&PathStep>> = match path {
        [root, .., current] if path.len() > 2 => vec![Some(root), None, Some(current)],
        _ => whole.clone(),
    };
    let with_status = format!("{status}{key}");
    let layouts = [
        (&whole, with_status.as_str(), true),
        (&elided, with_status.as_str(), true),
        (&whole, status, true),
        (&elided, status, true),
        (&elided, status, false),
    ];
    let last = layouts.len() - 1;
    for (index, (steps, tail, with_agents)) in layouts.into_iter().enumerate() {
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
        // The last layout keeps other names readable too, unless the terminal
        // is too narrow to give the current name a fair share.
        let readable = names[..names.len().saturating_sub(1)]
            .iter()
            .map(|name| name.width().min(MIN_OTHER))
            .sum::<usize>();
        let floor = if index != last || room >= readable + MIN_OTHER * 2 {
            MIN_OTHER
        } else {
            MIN_NAME_CELLS
        };
        let budgets = share_current_last(&names, room, floor);
        let fits = names
            .iter()
            .zip(&budgets)
            .enumerate()
            .all(|(at, (name, budget))| {
                *budget
                    >= if at + 1 == names.len() {
                        name.width()
                    } else {
                        name.width().min(floor)
                    }
            });
        if !fits && index != last {
            continue;
        }
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut named = 0;
        let current = steps.len().saturating_sub(1);
        for (position, step) in steps.iter().enumerate() {
            if position > 0 {
                spans.push(PATH_SEPARATOR.dim());
            }
            match step {
                Some(step) => {
                    if with_agents {
                        spans.push(agent_label(step, true).dim());
                    }
                    let name = fit_name(&step.name, budgets[named]);
                    named += 1;
                    spans.push(if position == current {
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
    Line::default()
}

fn agent_label(step: &PathStep, with_agents: bool) -> String {
    if with_agents {
        format!("{} · ", step.agent)
    } else {
        String::new()
    }
}

/// Share `room` cells so the last name (the current session) is cut last:
/// every other name keeps up to `floor` cells, the current name takes what it
/// needs from the rest, and the others share what it leaves.
fn share_current_last(names: &[&str], room: usize, floor: usize) -> Vec<usize> {
    let Some((current, others)) = names.split_last() else {
        return Vec::new();
    };
    let reserved = others
        .iter()
        .map(|name| name.width().min(floor))
        .sum::<usize>();
    let current_budget = current.width().min(room.saturating_sub(reserved));
    let mut budgets = share(others, room - current_budget);
    budgets.push(current_budget);
    budgets
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
        .map(|(name, budget)| fit_name(name, budget))
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
            ("three levels", 120, breadcrumb_line(&three, "", KEY, 120)),
            ("three levels", 78, breadcrumb_line(&three, "", KEY, 78)),
            ("shared prefix", 120, breadcrumb_line(&shared, "", KEY, 120)),
            ("shared prefix", 78, breadcrumb_line(&shared, "", KEY, 78)),
            ("long names", 118, breadcrumb_line(&long, "", KEY, 118)),
            ("long names", 78, breadcrumb_line(&long, "", KEY, 78)),
            ("long names", 40, breadcrumb_line(&long, "", KEY, 40)),
        ]));
    }

    const SHARED_PARENT: &str = "shared prefix for the parser review: parent side";
    const SHARED_CHILD: &str =
        "shared prefix for the parser review: child side mock:request-permission:z";

    #[test]
    fn names_sharing_a_long_prefix_stay_apart() {
        let crumb = text(&breadcrumb_line(
            &[
                step("mock-model", SHARED_PARENT),
                step("mock-model-alt", SHARED_CHILD),
            ],
            " · waiting on you",
            " · alt+up parent",
            120,
        ));
        assert!(crumb.contains("parent side"), "{crumb}");
        assert!(crumb.contains("permission:z"), "{crumb}");
        let waiting = text(&waiting_line(
            &[SHARED_PARENT.to_string(), SHARED_CHILD.to_string()],
            None,
            0,
            Some("alt + down"),
            120,
        ));
        assert!(waiting.contains("parent side ›"), "{waiting}");
        assert!(waiting.contains("permission:z is waiting"), "{waiting}");
    }

    #[test]
    fn a_cut_name_keeps_its_start_and_end() {
        assert_eq!(fit_name("Review the parser", 40), "Review the parser");
        let cut = fit_name("shared prefix for the parser review: parent side", 20);
        assert!(cut.width() <= 20, "{cut}");
        assert!(cut.starts_with("shared"), "{cut}");
        assert!(cut.ends_with("side"), "{cut}");
        assert!(cut.contains('…'), "{cut}");
        for (wide, cells) in [("界界界界界界界界", 7), ("👩‍💻👩‍💻👩‍💻👩‍💻", 5), ("abcdef", 1)]
        {
            let cut = fit_name(wide, cells);
            assert!(cut.width() <= cells, "{wide} @{cells}: {cut}");
            assert!(cut.contains('…'), "{wide} @{cells}: {cut}");
        }
        let joined = fit_name("👩‍💻👩‍💻👩‍💻👩‍💻", 5);
        assert_eq!(
            joined.replace("👩‍💻", "").replace('…', ""),
            "",
            "graphemes stay whole: {joined:?}"
        );
    }

    #[test]
    fn a_long_current_name_still_leaves_the_root_readable() {
        let path = [
            step("mock-model", "Ship tree-row picker"),
            step(
                "mock-model-alt",
                "mock:request-permission:a delegated task line that runs on for well over eighty cells in total",
            ),
        ];
        let line = text(&breadcrumb_line(
            &path,
            " · waiting on you",
            " · alt+up parent",
            119,
        ));
        let root = line.split(" › ").next().unwrap_or_default();
        assert!(root.width() >= 8 && root.contains("Ship"), "{line}");
        assert!(
            line.contains("eighty cells in total · waiting on you"),
            "{line}"
        );
    }

    #[test]
    fn the_key_gives_way_before_the_current_name_is_cut() {
        let grandchild = [
            step("mock-model", "mock:delegate mock-model mock:delegate mo"),
            step("mock-model", "mock:delegate mock-model mock:request-pe"),
            step("mock-model", "mock:request-permission:x"),
        ];
        let status = " · waiting on you";
        let key = " · alt+up parent";
        let narrow = text(&breadcrumb_line(&grandchild, status, key, 79));
        assert!(
            narrow.contains("mock:request-permission:x · waiting on you"),
            "{narrow}"
        );
        assert!(!narrow.contains("alt+up"), "{narrow}");
        let wide = text(&breadcrumb_line(&grandchild, status, key, 119));
        assert!(
            wide.ends_with("mock:request-permission:x · waiting on you · alt+up parent"),
            "{wide}"
        );
    }

    #[test]
    fn a_breadcrumb_never_drops_its_root_or_its_current_session() {
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
            let line = text(&breadcrumb_line(&long, "", KEY, width));
            assert!(line.contains("A parent"), "@{width}: {line}");
            assert!(
                line.contains("A grandchild whose title is also very long here"),
                "@{width}: {line}"
            );
        }
        let wide = text(&breadcrumb_line(&long, "", KEY, 118));
        assert!(wide.ends_with(KEY), "{wide}");
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
                breadcrumb_line(&long, "", KEY, width),
                breadcrumb_line(&long[..1], "", KEY, width),
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

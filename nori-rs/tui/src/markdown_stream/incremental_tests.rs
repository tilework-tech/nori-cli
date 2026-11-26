//! Guards for incremental streaming: committed output must match a collector that re-renders the
//! whole buffer on every commit, and streaming cost must grow linearly with message length.

use std::time::Duration;
use std::time::Instant;

use pretty_assertions::assert_eq;
use ratatui::text::Line;

use super::MarkdownStreamCollector;
use crate::markdown;
use crate::markdown_render::committable_prefix_len;

/// Collector that renders the entire buffer on every commit. Its output defines the expected
/// streamed transcript.
struct WholeBufferCollector {
    buffer: String,
    committed_line_count: usize,
    width: Option<usize>,
}

impl WholeBufferCollector {
    fn new(width: Option<usize>) -> Self {
        Self {
            buffer: String::new(),
            committed_line_count: 0,
            width,
        }
    }

    fn commit_complete_lines(&mut self) -> Vec<Line<'static>> {
        let Some(last_newline_idx) = self.buffer.rfind('\n') else {
            return Vec::new();
        };
        let source = &self.buffer[..=last_newline_idx];
        let source = &source[..committable_prefix_len(source)];
        let mut rendered = Vec::new();
        markdown::append_markdown(source, self.width, &mut rendered);
        let mut complete_line_count = rendered.len();
        if complete_line_count > 0
            && crate::render::line_utils::is_blank_line_spaces_only(
                &rendered[complete_line_count - 1],
            )
        {
            complete_line_count -= 1;
        }
        if self.committed_line_count >= complete_line_count {
            return Vec::new();
        }
        let out = rendered[self.committed_line_count..complete_line_count].to_vec();
        self.committed_line_count = complete_line_count;
        out
    }

    fn finalize(&mut self) -> Vec<Line<'static>> {
        let mut source = self.buffer.clone();
        if !source.ends_with('\n') {
            source.push('\n');
        }
        let mut rendered = Vec::new();
        markdown::append_markdown(&source, self.width, &mut rendered);
        if self.committed_line_count >= rendered.len() {
            Vec::new()
        } else {
            rendered[self.committed_line_count..].to_vec()
        }
    }
}

/// Deterministic linear congruential generator, so failures reproduce from the printed seed.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, bound: i64) -> i64 {
        (self.next() % bound as u64) as i64
    }

    fn chance(&mut self, percent: i64) -> bool {
        self.below(100) < percent
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len() as i64) as usize]
    }
}

const WORDS: &[&str] = &[
    "alpha",
    "beta",
    "gamma",
    "delta",
    "streaming",
    "render",
    "commit",
    "汉字",
    "🙂",
    "naïve",
    "x",
    "a longer phrase that wraps",
];

const INLINE: &[&str] = &[
    "*emphasis*",
    "**strong**",
    "`code span`",
    "[link](https://example.com)",
    "~~struck~~",
    "<span>",
    "&amp;",
    "a | b",
    "*open emphasis",
    "close emphasis*",
    "`open code",
    "close code`",
    "[open link",
    "close](https://example.com)",
    "[ref]",
    "<https://example.com>",
    "_under_",
    "\\*escaped",
];

fn words(rng: &mut Lcg, inline_percent: i64) -> String {
    let count = 1 + rng.below(8);
    (0..count)
        .map(|_| {
            if rng.chance(inline_percent) {
                rng.pick(INLINE)
            } else {
                rng.pick(WORDS)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn paragraph(rng: &mut Lcg, out: &mut String, indent: &str, inline_percent: i64) {
    let lines = 1 + rng.below(5);
    for index in 0..lines {
        if index > 0 && rng.chance(8) {
            // Lines that would start a block on their own but continue a paragraph here.
            out.push_str(indent);
            out.push_str(rng.pick(&["2. not a list", "    indented", "<div>", "# not", "- item"]));
            out.push('\n');
            continue;
        }
        out.push_str(indent);
        out.push_str(&words(rng, inline_percent));
        if rng.chance(8) {
            out.push_str(rng.pick(&["  ", "\\", " "]));
        }
        out.push('\n');
    }
    if rng.chance(6) {
        out.push_str(indent);
        out.push_str(rng.pick(&["---", "===", "-", "  ==="]));
        out.push('\n');
    }
}

fn list(rng: &mut Lcg, out: &mut String, ordered: bool, depth: i64) {
    let items = 1 + rng.below(5);
    let loose = rng.chance(30);
    let indent = "   ".repeat(depth as usize);
    for index in 0..items {
        let marker = if ordered {
            format!("{}. ", index + 1)
        } else {
            rng.pick(&["- ", "* ", "+ "]).to_string()
        };
        out.push_str(&indent);
        out.push_str(&marker);
        out.push_str(&words(rng, 15));
        out.push('\n');
        let continuation = format!("{indent}{}", " ".repeat(marker.len()));
        if rng.chance(25) {
            if loose {
                out.push('\n');
            }
            paragraph(rng, out, &continuation, 10);
        }
        if rng.chance(15) {
            out.push_str(&continuation);
            out.push_str("```\n");
            out.push_str(&continuation);
            out.push_str("code in item\n\n");
            out.push_str(&continuation);
            out.push_str("```\n");
        }
        if depth < 2 && rng.chance(20) {
            let nested_ordered = rng.chance(50);
            list(rng, out, nested_ordered, depth + 1);
        }
        if loose {
            out.push('\n');
        }
    }
}

fn table(rng: &mut Lcg, out: &mut String) {
    out.push_str("| Name | Value |\n| --- | :-: |\n");
    for _ in 0..rng.below(4) {
        out.push_str(&format!("| {} | {} |\n", words(rng, 0), words(rng, 20)));
    }
}

fn block(rng: &mut Lcg, out: &mut String) {
    match rng.below(16) {
        0..=4 => paragraph(rng, out, "", 25),
        5 => out.push_str(&format!(
            "{} {}\n",
            rng.pick(&["#", "##", "###"]),
            words(rng, 20)
        )),
        6 => list(rng, out, false, 0),
        7 => list(rng, out, true, 0),
        8 => {
            let fence = rng.pick(&["```", "```rust", "~~~", "````text"]);
            out.push_str(fence);
            out.push('\n');
            for _ in 0..rng.below(5) {
                if rng.chance(30) {
                    out.push('\n');
                } else {
                    out.push_str(rng.pick(&[
                        "let x = 1;",
                        "| a | b |",
                        "    indented",
                        "```inner",
                    ]));
                    out.push('\n');
                }
            }
            if rng.chance(90) {
                out.push_str(&fence[..if fence.starts_with("````") { 4 } else { 3 }]);
                out.push('\n');
            }
        }
        9 => table(rng, out),
        10 => {
            for _ in 0..1 + rng.below(3) {
                out.push_str("> ");
                out.push_str(&words(rng, 15));
                out.push('\n');
            }
            if rng.chance(30) {
                out.push_str("lazy continuation\n");
            }
        }
        11 => out.push_str(rng.pick(&[
            "<div>\nhtml block\n</div>\n",
            "<pre>\n\nkept across blank lines\n\n</pre>\n",
            "<!-- comment\n\nstill comment -->\n",
        ])),
        12 => out.push_str(rng.pick(&["---\n", "***\n", "___\n"])),
        13 => out.push_str(rng.pick(&["[ref]: https://example.com/ref\n", "[other]: /x\n"])),
        14 => out.push_str("    indented code\n\n    more indented code\n"),
        _ => {
            paragraph(rng, out, "", 10);
            table(rng, out);
        }
    }
}

fn document(rng: &mut Lcg) -> String {
    let mut out = String::new();
    for _ in 0..1 + rng.below(12) {
        block(rng, &mut out);
        match rng.below(10) {
            0..=6 => out.push('\n'),
            7 => out.push_str("\n\n"),
            _ => {}
        }
    }
    if rng.chance(30) {
        out.pop();
    }
    out
}

fn chunks(rng: &mut Lcg, source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut target = 1 + rng.below(24);
    for character in source.chars() {
        current.push(character);
        if current.chars().count() as i64 >= target {
            out.push(std::mem::take(&mut current));
            target = 1 + rng.below(24);
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn stream_with_collector(deltas: &[String], width: Option<usize>) -> Vec<Line<'static>> {
    let mut collector = MarkdownStreamCollector::new(width);
    let mut out = Vec::new();
    for delta in deltas {
        collector.push_delta(delta);
        if delta.contains('\n') {
            out.extend(collector.commit_complete_lines());
        }
    }
    out.extend(collector.finalize_and_drain().0);
    out
}

fn stream_with_whole_buffer(deltas: &[String], width: Option<usize>) -> Vec<Line<'static>> {
    let mut collector = WholeBufferCollector::new(width);
    let mut out = Vec::new();
    for delta in deltas {
        collector.buffer.push_str(delta);
        if delta.contains('\n') {
            out.extend(collector.commit_complete_lines());
        }
    }
    out.extend(collector.finalize());
    out
}

#[test]
fn streamed_output_matches_whole_buffer_rendering_for_generated_documents() {
    for seed in 0..400 {
        let mut rng = Lcg(seed);
        let source = document(&mut rng);
        let deltas = chunks(&mut rng, &source);
        for width in [None, Some(24), Some(80)] {
            assert_eq!(
                stream_with_collector(&deltas, width),
                stream_with_whole_buffer(&deltas, width),
                "seed {seed}, width {width:?}, deltas {deltas:?}"
            );
        }
    }
}

#[test]
fn streamed_output_matches_whole_buffer_rendering_line_by_line() {
    for seed in 0..150 {
        let mut rng = Lcg(seed);
        let source = document(&mut rng);
        let deltas = source
            .split_inclusive('\n')
            .map(str::to_string)
            .collect::<Vec<_>>();
        assert_eq!(
            stream_with_collector(&deltas, Some(80)),
            stream_with_whole_buffer(&deltas, Some(80)),
            "seed {seed}, source:\n{source}"
        );
    }
}

#[test]
fn streamed_output_matches_full_render_for_plain_documents() {
    for seed in 0..150 {
        let mut rng = Lcg(seed);
        let mut source = String::new();
        for _ in 0..1 + rng.below(6) {
            paragraph(&mut rng, &mut source, "", 0);
            source.push('\n');
        }
        let deltas = chunks(&mut rng, &source);
        let mut full = Vec::new();
        markdown::append_markdown(&source, Some(80), &mut full);
        // Setext underlines and backslash hard breaks change lines that streaming already
        // committed, so only documents without them are expected to match the one-shot render.
        let rewrites_committed_lines = source
            .lines()
            .any(|line| matches!(line.trim(), "---" | "===" | "-") || line.ends_with('\\'));
        if !rewrites_committed_lines {
            assert_eq!(
                stream_with_collector(&deltas, Some(80)),
                full,
                "seed {seed}, source:\n{source}"
            );
        }
    }
}

fn stream_time(deltas: &[String]) -> Duration {
    let started = Instant::now();
    let mut collector = MarkdownStreamCollector::new(Some(80));
    for delta in deltas {
        collector.push_delta(delta);
        collector.commit_complete_lines();
    }
    collector.finalize_and_drain();
    started.elapsed()
}

/// Load from other tests only ever adds time, so the fastest of several
/// interleaved runs is the closest measure of the work itself. Linear work
/// doubles with twice the lines; the old quadratic collector quadrupled.
#[test]
fn streaming_cost_grows_linearly_with_message_length() {
    let lines = |count: i64| {
        (0..count)
            .map(|index| format!("synthetic reply {index}\n"))
            .collect::<Vec<_>>()
    };
    let (single_lines, double_lines) = (lines(4_000), lines(8_000));
    let mut single = Duration::MAX;
    let mut double = Duration::MAX;
    for _ in 0..5 {
        single = single.min(stream_time(&single_lines));
        double = double.min(stream_time(&double_lines));
    }
    assert!(
        double < single * 3,
        "streaming 8,000 lines took {double:?}, more than 3x the {single:?} for 4,000 lines"
    );
}

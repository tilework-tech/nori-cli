use diffy::Hunk;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line as RtLine;
use ratatui::text::Span as RtSpan;
use ratatui::widgets::Paragraph;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

use crate::exec_command::relativize_to_home;
use crate::render::Insets;
use crate::render::highlight::highlight_code_to_lines_for_path;
use crate::render::line_utils::prefix_lines;
use crate::render::renderable::ColumnRenderable;
use crate::render::renderable::InsetRenderable;
use crate::render::renderable::Renderable;
use codex_core::git_info::get_git_repo_root;
use codex_core::protocol::FileChange;

// ---------------------------------------------------------------------------
// Color-level and theme detection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffColorLevel {
    TrueColor,
    Ansi256,
    Ansi16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffTheme {
    Dark,
    Light,
}

// Hardcoded palette constants
const DARK_TC_ADD_BG: (u8, u8, u8) = (33, 58, 43);
const DARK_TC_DEL_BG: (u8, u8, u8) = (74, 34, 29);
const LIGHT_TC_ADD_BG: (u8, u8, u8) = (218, 251, 225);
const LIGHT_TC_DEL_BG: (u8, u8, u8) = (255, 235, 233);

const DARK_256_ADD_BG: u8 = 22;
const DARK_256_DEL_BG: u8 = 52;
const LIGHT_256_ADD_BG: u8 = 194;
const LIGHT_256_DEL_BG: u8 = 224;

fn diff_color_level() -> DiffColorLevel {
    let Some(level) = supports_color::on_cached(supports_color::Stream::Stdout) else {
        return DiffColorLevel::Ansi16;
    };
    if level.has_16m {
        DiffColorLevel::TrueColor
    } else if level.has_256 {
        DiffColorLevel::Ansi256
    } else {
        DiffColorLevel::Ansi16
    }
}

fn diff_theme() -> DiffTheme {
    match crate::terminal_palette::default_bg() {
        Some(bg) if crate::color::is_light(bg) => DiffTheme::Light,
        _ => DiffTheme::Dark,
    }
}

fn resolve_bg(theme: DiffTheme, level: DiffColorLevel, is_add: bool) -> Option<Color> {
    match level {
        DiffColorLevel::TrueColor => {
            let (r, g, b) = match (theme, is_add) {
                (DiffTheme::Dark, true) => DARK_TC_ADD_BG,
                (DiffTheme::Dark, false) => DARK_TC_DEL_BG,
                (DiffTheme::Light, true) => LIGHT_TC_ADD_BG,
                (DiffTheme::Light, false) => LIGHT_TC_DEL_BG,
            };
            #[allow(clippy::disallowed_methods)]
            Some(Color::Rgb(r, g, b))
        }
        DiffColorLevel::Ansi256 => {
            let idx = match (theme, is_add) {
                (DiffTheme::Dark, true) => DARK_256_ADD_BG,
                (DiffTheme::Dark, false) => DARK_256_DEL_BG,
                (DiffTheme::Light, true) => LIGHT_256_ADD_BG,
                (DiffTheme::Light, false) => LIGHT_256_DEL_BG,
            };
            #[allow(clippy::disallowed_methods)]
            Some(Color::Indexed(idx))
        }
        DiffColorLevel::Ansi16 => None,
    }
}

struct DiffRenderStyleContext {
    add_bg: Option<Color>,
    del_bg: Option<Color>,
}

impl DiffRenderStyleContext {
    fn new() -> Self {
        let theme = diff_theme();
        let level = diff_color_level();
        Self {
            add_bg: resolve_bg(theme, level, true),
            del_bg: resolve_bg(theme, level, false),
        }
    }

    #[cfg(test)]
    fn ansi16() -> Self {
        Self {
            add_bg: None,
            del_bg: None,
        }
    }
}

// Internal representation for diff line rendering
enum DiffLineType {
    Insert,
    Delete,
    Context,
}

#[derive(Clone, Copy)]
struct DiffLineLayout {
    width: usize,
    line_number_width: usize,
    outer_pad: usize,
}

pub struct DiffSummary {
    changes: HashMap<PathBuf, FileChange>,
    cwd: PathBuf,
}

impl DiffSummary {
    pub fn new(changes: HashMap<PathBuf, FileChange>, cwd: PathBuf) -> Self {
        Self { changes, cwd }
    }
}

impl Renderable for FileChange {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let mut lines = vec![];
        render_change(self, &mut lines, area.width as usize, None);
        Paragraph::new(lines).render(area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        let mut lines = vec![];
        render_change(self, &mut lines, width as usize, None);
        lines.len() as u16
    }
}

impl From<DiffSummary> for Box<dyn Renderable> {
    fn from(val: DiffSummary) -> Self {
        let mut rows: Vec<Box<dyn Renderable>> = vec![];

        for (i, row) in collect_rows(&val.changes).into_iter().enumerate() {
            if i > 0 {
                rows.push(Box::new(RtLine::from("")));
            }
            let mut path = RtLine::from(display_path_for(&row.path, &val.cwd));
            path.push_span(" ");
            path.extend(render_line_count_summary(row.added, row.removed));
            rows.push(Box::new(path));
            rows.push(Box::new(RtLine::from("")));
            rows.push(Box::new(InsetRenderable::new(
                Box::new(PathAwareFileChange {
                    path: row.path,
                    change: row.change,
                }) as Box<dyn Renderable>,
                Insets::tlbr(0, 2, 0, 0),
            )));
        }

        Box::new(ColumnRenderable::with(rows))
    }
}

struct PathAwareFileChange {
    path: PathBuf,
    change: FileChange,
}

impl Renderable for PathAwareFileChange {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let mut lines = vec![];
        render_change(
            &self.change,
            &mut lines,
            area.width as usize,
            Some(&self.path),
        );
        Paragraph::new(lines).render(area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        let mut lines = vec![];
        render_change(&self.change, &mut lines, width as usize, Some(&self.path));
        lines.len() as u16
    }
}

pub(crate) fn create_diff_summary(
    changes: &HashMap<PathBuf, FileChange>,
    cwd: &Path,
    wrap_cols: usize,
) -> Vec<RtLine<'static>> {
    let rows = collect_rows(changes);
    render_changes_block(rows, wrap_cols, cwd)
}

// Shared row for per-file presentation
#[derive(Clone)]
struct Row {
    #[allow(dead_code)]
    path: PathBuf,
    move_path: Option<PathBuf>,
    added: usize,
    removed: usize,
    change: FileChange,
}

fn collect_rows(changes: &HashMap<PathBuf, FileChange>) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    for (path, change) in changes.iter() {
        let (added, removed) = match change {
            FileChange::Add { content } => (content.lines().count(), 0),
            FileChange::Delete { content } => (0, content.lines().count()),
            FileChange::Update { unified_diff, .. } => calculate_add_remove_from_diff(unified_diff),
        };
        let move_path = match change {
            FileChange::Update {
                move_path: Some(new),
                ..
            } => Some(new.clone()),
            _ => None,
        };
        rows.push(Row {
            path: path.clone(),
            move_path,
            added,
            removed,
            change: change.clone(),
        });
    }
    rows.sort_by_key(|r| r.path.clone());
    rows
}

fn render_line_count_summary(added: usize, removed: usize) -> Vec<RtSpan<'static>> {
    let mut spans = Vec::new();
    spans.push("(".into());
    spans.push(format!("+{added}").green());
    spans.push(" ".into());
    spans.push(format!("-{removed}").red());
    spans.push(")".into());
    spans
}

fn render_changes_block(rows: Vec<Row>, wrap_cols: usize, cwd: &Path) -> Vec<RtLine<'static>> {
    let mut out: Vec<RtLine<'static>> = Vec::new();

    let render_path = |row: &Row| -> Vec<RtSpan<'static>> {
        let mut spans = Vec::new();
        spans.push(display_path_for(&row.path, cwd).into());
        if let Some(move_path) = &row.move_path {
            spans.push(format!(" → {}", display_path_for(move_path, cwd)).into());
        }
        spans
    };

    // Header
    let total_added: usize = rows.iter().map(|r| r.added).sum();
    let total_removed: usize = rows.iter().map(|r| r.removed).sum();
    let file_count = rows.len();
    let noun = if file_count == 1 { "file" } else { "files" };
    let mut header_spans: Vec<RtSpan<'static>> = vec!["• ".dim()];
    if let [row] = &rows[..] {
        let verb = match &row.change {
            FileChange::Add { .. } => "Added",
            FileChange::Delete { .. } => "Deleted",
            _ => "Edited",
        };
        header_spans.push(verb.bold());
        header_spans.push(" ".into());
        header_spans.extend(render_path(row));
        header_spans.push(" ".into());
        header_spans.extend(render_line_count_summary(row.added, row.removed));
    } else {
        header_spans.push("Edited".bold());
        header_spans.push(format!(" {file_count} {noun} ").into());
        header_spans.extend(render_line_count_summary(total_added, total_removed));
    }
    out.push(RtLine::from(header_spans));

    for (idx, r) in rows.into_iter().enumerate() {
        // Insert a blank separator between file chunks (except before the first)
        if idx > 0 {
            out.push("".into());
        }
        // File header line (skip when single-file header already shows the name)
        let skip_file_header = file_count == 1;
        if !skip_file_header {
            let mut header: Vec<RtSpan<'static>> = Vec::new();
            header.push("  └ ".dim());
            header.extend(render_path(&r));
            header.push(" ".into());
            header.extend(render_line_count_summary(r.added, r.removed));
            out.push(RtLine::from(header));
        }

        let mut lines = vec![];
        let prefix_width = 4;
        let ctx = DiffRenderStyleContext::new();
        render_change_with_ctx(
            &r.change,
            &mut lines,
            wrap_cols - prefix_width,
            prefix_width,
            &ctx,
            Some(&r.path),
        );
        out.extend(prefix_lines(lines, "    ".into(), "    ".into()));
    }

    out
}

fn render_change(
    change: &FileChange,
    out: &mut Vec<RtLine<'static>>,
    width: usize,
    path: Option<&Path>,
) {
    render_change_with_ctx(change, out, width, 0, &DiffRenderStyleContext::new(), path);
}

fn render_change_with_ctx(
    change: &FileChange,
    out: &mut Vec<RtLine<'static>>,
    width: usize,
    outer_pad: usize,
    ctx: &DiffRenderStyleContext,
    path: Option<&Path>,
) {
    match change {
        FileChange::Add { content } => {
            let line_number_width = line_number_width(content.lines().count());
            for (i, raw) in content.lines().enumerate() {
                out.extend(push_wrapped_diff_line_maybe_path(
                    i + 1,
                    DiffLineType::Insert,
                    raw,
                    DiffLineLayout {
                        width,
                        line_number_width,
                        outer_pad,
                    },
                    ctx,
                    path,
                ));
            }
        }
        FileChange::Delete { content } => {
            let line_number_width = line_number_width(content.lines().count());
            for (i, raw) in content.lines().enumerate() {
                out.extend(push_wrapped_diff_line_maybe_path(
                    i + 1,
                    DiffLineType::Delete,
                    raw,
                    DiffLineLayout {
                        width,
                        line_number_width,
                        outer_pad,
                    },
                    ctx,
                    path,
                ));
            }
        }
        FileChange::Update { unified_diff, .. } => {
            if let Ok(patch) = diffy::Patch::from_str(unified_diff) {
                let mut max_line_number = 0;
                for h in patch.hunks() {
                    let mut old_ln = h.old_range().start();
                    let mut new_ln = h.new_range().start();
                    for l in h.lines() {
                        match l {
                            diffy::Line::Insert(_) => {
                                max_line_number = max_line_number.max(new_ln);
                                new_ln += 1;
                            }
                            diffy::Line::Delete(_) => {
                                max_line_number = max_line_number.max(old_ln);
                                old_ln += 1;
                            }
                            diffy::Line::Context(_) => {
                                max_line_number = max_line_number.max(new_ln);
                                old_ln += 1;
                                new_ln += 1;
                            }
                        }
                    }
                }
                let line_number_width = line_number_width(max_line_number);
                let mut is_first_hunk = true;
                for h in patch.hunks() {
                    if !is_first_hunk {
                        let spacer = format!("{:width$} ", "", width = line_number_width.max(1));
                        let spacer_span = RtSpan::styled(spacer, style_gutter());
                        out.push(RtLine::from(vec![spacer_span, "⋮".dim()]));
                    }
                    is_first_hunk = false;

                    let mut old_ln = h.old_range().start();
                    let mut new_ln = h.new_range().start();
                    for l in h.lines() {
                        match l {
                            diffy::Line::Insert(text) => {
                                let s = text.trim_end_matches('\n');
                                out.extend(push_wrapped_diff_line_maybe_path(
                                    new_ln,
                                    DiffLineType::Insert,
                                    s,
                                    DiffLineLayout {
                                        width,
                                        line_number_width,
                                        outer_pad,
                                    },
                                    ctx,
                                    path,
                                ));
                                new_ln += 1;
                            }
                            diffy::Line::Delete(text) => {
                                let s = text.trim_end_matches('\n');
                                out.extend(push_wrapped_diff_line_maybe_path(
                                    old_ln,
                                    DiffLineType::Delete,
                                    s,
                                    DiffLineLayout {
                                        width,
                                        line_number_width,
                                        outer_pad,
                                    },
                                    ctx,
                                    path,
                                ));
                                old_ln += 1;
                            }
                            diffy::Line::Context(text) => {
                                let s = text.trim_end_matches('\n');
                                out.extend(push_wrapped_diff_line_maybe_path(
                                    new_ln,
                                    DiffLineType::Context,
                                    s,
                                    DiffLineLayout {
                                        width,
                                        line_number_width,
                                        outer_pad,
                                    },
                                    ctx,
                                    path,
                                ));
                                old_ln += 1;
                                new_ln += 1;
                            }
                        }
                    }
                }
            }
        }
    }
}

pub(crate) fn display_path_for(path: &Path, cwd: &Path) -> String {
    let path_in_same_repo = match (get_git_repo_root(cwd), get_git_repo_root(path)) {
        (Some(cwd_repo), Some(path_repo)) => cwd_repo == path_repo,
        _ => false,
    };
    let chosen = if path_in_same_repo {
        pathdiff::diff_paths(path, cwd).unwrap_or_else(|| path.to_path_buf())
    } else {
        relativize_to_home(path)
            .map(|p| PathBuf::from_iter([Path::new("~"), p.as_path()]))
            .unwrap_or_else(|| path.to_path_buf())
    };
    chosen.display().to_string()
}

fn calculate_add_remove_from_diff(diff: &str) -> (usize, usize) {
    if let Ok(patch) = diffy::Patch::from_str(diff) {
        patch
            .hunks()
            .iter()
            .flat_map(Hunk::lines)
            .fold((0, 0), |(a, d), l| match l {
                diffy::Line::Insert(_) => (a + 1, d),
                diffy::Line::Delete(_) => (a, d + 1),
                diffy::Line::Context(_) => (a, d),
            })
    } else {
        // For unparsable diffs, return 0 for both counts.
        (0, 0)
    }
}

fn push_wrapped_diff_line(
    line_number: usize,
    kind: DiffLineType,
    text: &str,
    layout: DiffLineLayout,
    ctx: &DiffRenderStyleContext,
) -> Vec<RtLine<'static>> {
    push_wrapped_diff_line_for_path(line_number, kind, text, Path::new(""), layout, ctx)
}

fn push_wrapped_diff_line_maybe_path(
    line_number: usize,
    kind: DiffLineType,
    text: &str,
    layout: DiffLineLayout,
    ctx: &DiffRenderStyleContext,
    path: Option<&Path>,
) -> Vec<RtLine<'static>> {
    match path {
        Some(path) => push_wrapped_diff_line_for_path(line_number, kind, text, path, layout, ctx),
        None => push_wrapped_diff_line(line_number, kind, text, layout, ctx),
    }
}

fn push_wrapped_diff_line_for_path(
    line_number: usize,
    kind: DiffLineType,
    text: &str,
    path: &Path,
    layout: DiffLineLayout,
    ctx: &DiffRenderStyleContext,
) -> Vec<RtLine<'static>> {
    let ln_str = line_number.to_string();
    let mut remaining_text: &str = text;

    // Reserve a fixed number of spaces (equal to the widest line number plus a
    // trailing spacer) so the sign column stays aligned across the diff block.
    let gutter_width = layout.line_number_width.max(1);
    let prefix_cols = gutter_width + 1;

    let mut first = true;
    let (sign_char, line_style) = match kind {
        DiffLineType::Insert => ('+', style_add(ctx)),
        DiffLineType::Delete => ('-', style_del(ctx)),
        DiffLineType::Context => (' ', style_context()),
    };

    // Build a line-level background style so the bg extends edge-to-edge.
    let line_bg_style = match kind {
        DiffLineType::Insert => ctx.add_bg.map(|bg| Style::default().bg(bg)),
        DiffLineType::Delete => ctx.del_bg.map(|bg| Style::default().bg(bg)),
        DiffLineType::Context => None,
    };

    let mut lines: Vec<RtLine<'static>> = Vec::new();

    loop {
        // Fit the content for the current terminal row:
        // compute how many columns are available after the prefix, then split
        // at a UTF-8 character boundary so this row's chunk fits exactly.
        let available_content_cols = layout.width.saturating_sub(prefix_cols + 1).max(1);
        let split_at_byte_index = remaining_text
            .char_indices()
            .nth(available_content_cols)
            .map(|(i, _)| i)
            .unwrap_or_else(|| remaining_text.len());
        let (chunk, rest) = remaining_text.split_at(split_at_byte_index);
        remaining_text = rest;

        let (gutter_span, sign_span, content_spans, used_cols) = if first {
            let gutter = format!("{ln_str:>gutter_width$} ");
            let sign = sign_char.to_string();
            let content_spans = highlighted_diff_content_spans(chunk, path, line_style);
            let cols = gutter.len() + sign.len() + chunk.len();
            first = false;
            (gutter, Some(sign), content_spans, cols)
        } else {
            let gutter = format!("{:gutter_width$}  ", "");
            let content_spans = highlighted_diff_content_spans(chunk, path, line_style);
            let cols = gutter.len() + chunk.len();
            (gutter, None, content_spans, cols)
        };

        // When a background tint is active, apply it to every span (including
        // gutter) and pad with trailing spaces so the color fills the complete
        // terminal width from left edge to right edge.
        let line = if let Some(bg_style) = line_bg_style {
            let gutter_style = style_gutter().patch(bg_style);
            let content_style = line_style.patch(bg_style);
            let mut spans = vec![RtSpan::styled(gutter_span, gutter_style)];
            if let Some(sign) = sign_span {
                spans.push(RtSpan::styled(sign, content_style));
            }
            spans.extend(content_spans.into_iter().map(|span| {
                let style = span.style.patch(bg_style);
                RtSpan::styled(span.content.into_owned(), style)
            }));
            let pad = (layout.width + layout.outer_pad).saturating_sub(used_cols);
            if pad > 0 {
                spans.push(RtSpan::styled(" ".repeat(pad), bg_style));
            }
            RtLine::from(spans).style(bg_style)
        } else {
            let mut spans = vec![RtSpan::styled(gutter_span, style_gutter())];
            if let Some(sign) = sign_span {
                spans.push(RtSpan::styled(sign, line_style));
            }
            spans.extend(content_spans);
            RtLine::from(spans)
        };

        lines.push(line);

        if remaining_text.is_empty() {
            break;
        }
    }
    lines
}

fn highlighted_diff_content_spans(
    chunk: &str,
    path: &Path,
    fallback_style: Style,
) -> Vec<RtSpan<'static>> {
    if chunk.is_empty() {
        return vec![RtSpan::styled(String::new(), fallback_style)];
    }

    let highlighted = highlight_code_to_lines_for_path(chunk, path);
    let Some(line) = highlighted.into_iter().next() else {
        return vec![RtSpan::styled(chunk.to_string(), fallback_style)];
    };

    let has_syntax_style = line
        .spans
        .iter()
        .any(|span| span.style.fg.is_some() && span.style.fg != Some(Color::default()));
    if has_syntax_style {
        line.spans
    } else {
        vec![RtSpan::styled(chunk.to_string(), fallback_style)]
    }
}

fn line_number_width(max_line_number: usize) -> usize {
    if max_line_number == 0 {
        1
    } else {
        max_line_number.to_string().len()
    }
}

fn style_gutter() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

fn style_context() -> Style {
    Style::default()
}

fn style_add(ctx: &DiffRenderStyleContext) -> Style {
    let mut s = Style::default().fg(Color::Green);
    if let Some(bg) = ctx.add_bg {
        s = s.bg(bg);
    }
    s
}

fn style_del(ctx: &DiffRenderStyleContext) -> Style {
    let mut s = Style::default().fg(Color::Red);
    if let Some(bg) = ctx.del_bg {
        s = s.bg(bg);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::text::Text;
    use ratatui::widgets::Paragraph;
    use ratatui::widgets::WidgetRef;
    use ratatui::widgets::Wrap;
    fn diff_summary_for_tests(changes: &HashMap<PathBuf, FileChange>) -> Vec<RtLine<'static>> {
        create_diff_summary(changes, &PathBuf::from("/"), 80)
    }

    fn snapshot_lines(name: &str, lines: Vec<RtLine<'static>>, width: u16, height: u16) {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        terminal
            .draw(|f| {
                Paragraph::new(Text::from(lines))
                    .wrap(Wrap { trim: false })
                    .render_ref(f.area(), f.buffer_mut())
            })
            .expect("draw");
        assert_snapshot!(name, terminal.backend());
    }

    fn snapshot_lines_text(name: &str, lines: &[RtLine<'static>]) {
        // Convert Lines to plain text rows and trim trailing spaces so it's
        // easier to validate indentation visually in snapshots.
        let text = lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .map(|s| s.trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert_snapshot!(name, text);
    }

    #[test]
    fn ui_snapshot_wrap_behavior_insert() {
        // Narrow width to force wrapping within our diff line rendering
        let long_line = "this is a very long line that should wrap across multiple terminal columns and continue";
        let ctx = DiffRenderStyleContext::ansi16();

        // Call the wrapping function directly so we can precisely control the width
        let lines = push_wrapped_diff_line(
            1,
            DiffLineType::Insert,
            long_line,
            DiffLineLayout {
                width: 80,
                line_number_width: line_number_width(1),
                outer_pad: 0,
            },
            &ctx,
        );

        // Render into a small terminal to capture the visual layout
        snapshot_lines("wrap_behavior_insert", lines, 90, 8);
    }

    #[test]
    fn ui_snapshot_apply_update_block() {
        let mut changes: HashMap<PathBuf, FileChange> = HashMap::new();
        let original = "line one\nline two\nline three\n";
        let modified = "line one\nline two changed\nline three\n";
        let patch = diffy::create_patch(original, modified).to_string();

        changes.insert(
            PathBuf::from("example.txt"),
            FileChange::Update {
                unified_diff: patch,
                move_path: None,
            },
        );

        let lines = diff_summary_for_tests(&changes);

        snapshot_lines("apply_update_block", lines, 80, 12);
    }

    #[test]
    fn ui_snapshot_apply_update_with_rename_block() {
        let mut changes: HashMap<PathBuf, FileChange> = HashMap::new();
        let original = "A\nB\nC\n";
        let modified = "A\nB changed\nC\n";
        let patch = diffy::create_patch(original, modified).to_string();

        changes.insert(
            PathBuf::from("old_name.rs"),
            FileChange::Update {
                unified_diff: patch,
                move_path: Some(PathBuf::from("new_name.rs")),
            },
        );

        let lines = diff_summary_for_tests(&changes);

        snapshot_lines("apply_update_with_rename_block", lines, 80, 12);
    }

    #[test]
    fn ui_snapshot_apply_multiple_files_block() {
        // Two files: one update and one add, to exercise combined header and per-file rows
        let mut changes: HashMap<PathBuf, FileChange> = HashMap::new();

        // File a.txt: single-line replacement (one delete, one insert)
        let patch_a = diffy::create_patch("one\n", "one changed\n").to_string();
        changes.insert(
            PathBuf::from("a.txt"),
            FileChange::Update {
                unified_diff: patch_a,
                move_path: None,
            },
        );

        // File b.txt: newly added with one line
        changes.insert(
            PathBuf::from("b.txt"),
            FileChange::Add {
                content: "new\n".to_string(),
            },
        );

        let lines = diff_summary_for_tests(&changes);

        snapshot_lines("apply_multiple_files_block", lines, 80, 14);
    }

    #[test]
    fn ui_snapshot_apply_add_block() {
        let mut changes: HashMap<PathBuf, FileChange> = HashMap::new();
        changes.insert(
            PathBuf::from("new_file.txt"),
            FileChange::Add {
                content: "alpha\nbeta\n".to_string(),
            },
        );

        let lines = diff_summary_for_tests(&changes);

        snapshot_lines("apply_add_block", lines, 80, 10);
    }

    #[test]
    fn ui_snapshot_apply_delete_block() {
        // Write a temporary file so the delete renderer can read original content
        let tmp_path = PathBuf::from("tmp_delete_example.txt");
        std::fs::write(&tmp_path, "first\nsecond\nthird\n").expect("write tmp file");

        let mut changes: HashMap<PathBuf, FileChange> = HashMap::new();
        changes.insert(
            tmp_path.clone(),
            FileChange::Delete {
                content: "first\nsecond\nthird\n".to_string(),
            },
        );

        let lines = diff_summary_for_tests(&changes);

        // Cleanup best-effort; rendering has already read the file
        let _ = std::fs::remove_file(&tmp_path);

        snapshot_lines("apply_delete_block", lines, 80, 12);
    }

    #[test]
    fn ui_snapshot_apply_update_block_wraps_long_lines() {
        // Create a patch with a long modified line to force wrapping
        let original = "line 1\nshort\nline 3\n";
        let modified = "line 1\nshort this_is_a_very_long_modified_line_that_should_wrap_across_multiple_terminal_columns_and_continue_even_further_beyond_eighty_columns_to_force_multiple_wraps\nline 3\n";
        let patch = diffy::create_patch(original, modified).to_string();

        let mut changes: HashMap<PathBuf, FileChange> = HashMap::new();
        changes.insert(
            PathBuf::from("long_example.txt"),
            FileChange::Update {
                unified_diff: patch,
                move_path: None,
            },
        );

        let lines = create_diff_summary(&changes, &PathBuf::from("/"), 72);

        // Render with backend width wider than wrap width to avoid Paragraph auto-wrap.
        snapshot_lines("apply_update_block_wraps_long_lines", lines, 80, 12);
    }

    #[test]
    fn ui_snapshot_apply_update_block_wraps_long_lines_text() {
        // This mirrors the desired layout example: sign only on first inserted line,
        // subsequent wrapped pieces start aligned under the line number gutter.
        let original = "1\n2\n3\n4\n";
        let modified = "1\nadded long line which wraps and_if_there_is_a_long_token_it_will_be_broken\n3\n4 context line which also wraps across\n";
        let patch = diffy::create_patch(original, modified).to_string();

        let mut changes: HashMap<PathBuf, FileChange> = HashMap::new();
        changes.insert(
            PathBuf::from("wrap_demo.txt"),
            FileChange::Update {
                unified_diff: patch,
                move_path: None,
            },
        );

        let lines = create_diff_summary(&changes, &PathBuf::from("/"), 28);
        snapshot_lines_text("apply_update_block_wraps_long_lines_text", &lines);
    }

    #[test]
    fn ui_snapshot_apply_update_block_line_numbers_three_digits_text() {
        let original = (1..=110).map(|i| format!("line {i}\n")).collect::<String>();
        let modified = (1..=110)
            .map(|i| {
                if i == 100 {
                    format!("line {i} changed\n")
                } else {
                    format!("line {i}\n")
                }
            })
            .collect::<String>();
        let patch = diffy::create_patch(&original, &modified).to_string();

        let mut changes: HashMap<PathBuf, FileChange> = HashMap::new();
        changes.insert(
            PathBuf::from("hundreds.txt"),
            FileChange::Update {
                unified_diff: patch,
                move_path: None,
            },
        );

        let lines = create_diff_summary(&changes, &PathBuf::from("/"), 80);
        snapshot_lines_text("apply_update_block_line_numbers_three_digits_text", &lines);
    }

    #[test]
    fn ui_snapshot_apply_update_block_relativizes_path() {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        let abs_old = cwd.join("abs_old.rs");
        let abs_new = cwd.join("abs_new.rs");

        let original = "X\nY\n";
        let modified = "X changed\nY\n";
        let patch = diffy::create_patch(original, modified).to_string();

        let mut changes: HashMap<PathBuf, FileChange> = HashMap::new();
        changes.insert(
            abs_old,
            FileChange::Update {
                unified_diff: patch,
                move_path: Some(abs_new),
            },
        );

        let lines = create_diff_summary(&changes, &cwd, 80);

        snapshot_lines("apply_update_block_relativizes_path", lines, 80, 10);
    }

    #[test]
    fn diff_style_add_has_green_fg() {
        let ctx = DiffRenderStyleContext::ansi16();
        let style = style_add(&ctx);
        assert_eq!(style.fg, Some(Color::Green));
    }

    #[test]
    fn diff_style_del_has_red_fg() {
        let ctx = DiffRenderStyleContext::ansi16();
        let style = style_del(&ctx);
        assert_eq!(style.fg, Some(Color::Red));
    }

    #[test]
    fn diff_color_level_detection() {
        // Just verify it returns without panicking; the actual value depends on
        // the test runner's terminal capabilities.
        let _level = diff_color_level();
    }

    #[test]
    fn diff_theme_defaults_to_dark() {
        // In test environments, default_bg() returns None, so diff_theme()
        // should fall back to Dark.
        assert_eq!(diff_theme(), DiffTheme::Dark);
    }

    #[test]
    fn diff_bg_extends_through_prefix_lines_indent() {
        // Verifies that push_wrapped_diff_line produces lines where
        // prefix_lines can propagate the bg to the indent prefix,
        // giving edge-to-edge background highlighting.
        use ratatui::buffer::Buffer;
        use ratatui::layout::Rect;

        #[allow(clippy::disallowed_methods)]
        let green_bg = Color::Rgb(33, 58, 43);
        let total_width: u16 = 40;
        let prefix_width: usize = 4;
        let area = Rect::new(0, 0, total_width, 4);

        #[allow(clippy::disallowed_methods)]
        let ctx = DiffRenderStyleContext {
            add_bg: Some(Color::Rgb(33, 58, 43)),
            del_bg: Some(Color::Rgb(74, 34, 29)),
        };

        // Produce a diff line the same way production code does
        let diff_lines = push_wrapped_diff_line(
            1,
            DiffLineType::Insert,
            "hello",
            DiffLineLayout {
                width: (total_width as usize) - prefix_width * 2,
                line_number_width: 1,
                outer_pad: prefix_width,
            },
            &ctx,
        );

        // Apply prefix_lines just like render_changes_block does
        let lines = prefix_lines(diff_lines, "    ".into(), "    ".into());

        let mut buf = Buffer::empty(area);
        Paragraph::new(Text::from(lines)).render_ref(area, &mut buf);

        // Every cell in the row should have the green background
        for col in 0..total_width {
            assert_eq!(
                buf[(col, 0)].bg,
                green_bg,
                "col {col} should have bg — edge-to-edge fill requires all cells covered"
            );
        }
    }

    #[test]
    fn diff_bg_fills_full_width() {
        // With a truecolor context, each diff line's spans should cover the
        // full requested width so the background tint extends edge-to-edge.
        #[allow(clippy::disallowed_methods)]
        let ctx = DiffRenderStyleContext {
            add_bg: Some(Color::Rgb(33, 58, 43)),
            del_bg: Some(Color::Rgb(74, 34, 29)),
        };
        let content_width: usize = 36;
        let outer_pad: usize = 4;
        let total_width = content_width + outer_pad;
        let lines = push_wrapped_diff_line(
            1,
            DiffLineType::Insert,
            "hello",
            DiffLineLayout {
                width: content_width,
                line_number_width: 1,
                outer_pad,
            },
            &ctx,
        );
        assert_eq!(lines.len(), 1);
        let total_chars: usize = lines[0].spans.iter().map(|s| s.content.len()).sum();
        assert_eq!(
            total_chars, total_width,
            "expected line to fill {total_width} columns (content {content_width} + pad {outer_pad}), got {total_chars}"
        );
        // The line-level style should also carry the background
        assert!(
            lines[0].style.bg.is_some(),
            "expected line-level background style"
        );
        // Gutter span (first) should also have the background
        assert!(
            lines[0].spans[0].style.bg.is_some(),
            "expected gutter span to have background"
        );
    }

    #[test]
    fn diff_insert_content_uses_syntax_highlighting_when_path_is_known() {
        #[allow(clippy::disallowed_methods)]
        let ctx = DiffRenderStyleContext {
            add_bg: Some(Color::Rgb(33, 58, 43)),
            del_bg: Some(Color::Rgb(74, 34, 29)),
        };

        let lines = push_wrapped_diff_line_for_path(
            1,
            DiffLineType::Insert,
            "fn main() { println!(\"hello\"); }",
            Path::new("src/main.rs"),
            DiffLineLayout {
                width: 80,
                line_number_width: 1,
                outer_pad: 0,
            },
            &ctx,
        );

        assert!(
            lines[0].spans.iter().skip(2).any(|span| {
                span.style.fg.is_some()
                    && span.style.fg != Some(Color::Green)
                    && span.style.bg == ctx.add_bg
            }),
            "expected inserted Rust content to use syntax colors over the add background, got {:?}",
            lines[0].spans
        );
    }
}

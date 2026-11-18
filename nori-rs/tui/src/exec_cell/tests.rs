use std::path::PathBuf;

use insta::assert_snapshot;
use nori_config::ToolOutput;
use pretty_assertions::assert_eq;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::text::Line;

use crate::client_tool_cell::ClientToolCell;
use crate::history_cell::HistoryCell;
use crate::presentation::Artifact;
use crate::presentation::FileChange;
use crate::presentation::Invocation;
use crate::presentation::ToolKind;
use crate::presentation::ToolPhase;
use crate::presentation::ToolSnapshot;

const CHAINED: &str = "cd /home/dev/projects/nori && export RUST_LOG=debug && cargo build --release -p nori-tui && cargo test -p nori-tui -- --nocapture; ./scripts/notify.sh \"build finished\" && git status --short && docker compose -f deploy/compose.yml up -d";
const PIPELINE: &str =
    "git log --format='%an' origin/main..HEAD | sort | uniq -c | sort -rn | head -5";
const HEREDOC: &str = "python3 - <<'EOF'\nimport json\nwith open('report.json') as f:\n    data = json.load(f)\nprint(len(data['rows']), 'rows')\nEOF";

fn cwd() -> PathBuf {
    PathBuf::from("/home/dev/projects/nori")
}

fn execute(
    call_id: &str,
    phase: ToolPhase,
    command: &str,
    output: Option<(&str, i32)>,
) -> ToolSnapshot {
    ToolSnapshot {
        call_id: call_id.into(),
        title: command.into(),
        kind: ToolKind::Execute,
        phase,
        locations: vec![],
        invocation: Some(Invocation::Command {
            command: command.into(),
        }),
        artifacts: vec![],
        raw_input: Some(serde_json::json!({ "command": command })),
        raw_output: output
            .map(|(stdout, code)| serde_json::json!({ "stdout": stdout, "exit_code": code })),
        owner_request_id: None,
    }
}

fn acp(call_id: &str, kind: ToolKind, title: &str, invocation: Invocation) -> ToolSnapshot {
    ToolSnapshot {
        call_id: call_id.into(),
        title: title.into(),
        kind,
        phase: ToolPhase::Completed,
        locations: vec![],
        invocation: Some(invocation),
        artifacts: vec![],
        raw_input: None,
        raw_output: None,
        owner_request_id: None,
    }
}

fn cell(snapshot: ToolSnapshot) -> ClientToolCell {
    ClientToolCell::new(snapshot, cwd(), false).with_tool_output(ToolOutput::Compact)
}

fn explore_group() -> ClientToolCell {
    let mut cell = cell(acp(
        "read-1",
        ToolKind::Read,
        "Read src/main.rs",
        Invocation::Read {
            path: cwd().join("src/main.rs"),
        },
    ));
    cell.mark_exploring();
    cell.merge_exploring(acp(
        "search-1",
        ToolKind::Search,
        "Search TODO",
        Invocation::Search {
            query: Some("TODO".into()),
            path: Some(cwd().join("src")),
        },
    ));
    cell.merge_exploring(execute(
        "exec-1",
        ToolPhase::Failed,
        "grep -rn missing_symbol src/",
        Some(("", 1)),
    ));
    cell.merge_exploring(execute(
        "exec-2",
        ToolPhase::Completed,
        "rg -n 'fn render' src/ | sort -t: -k1,1 | uniq -c | sort -rn | head -20",
        Some((
            "     12 src/history_cell/mod.rs\n      4 src/exec_cell/render.rs",
            0,
        )),
    ));
    cell
}

fn text(lines: &[Line<'static>], width: u16) -> String {
    for line in lines {
        assert!(
            line.width() <= usize::from(width),
            "line wider than {width}: {line:?}"
        );
    }
    lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn compact(cell: &ClientToolCell) -> String {
    format!(
        "--- 120 ---\n{}\n--- 80 ---\n{}",
        text(&cell.display_lines(120), 120),
        text(&cell.display_lines(80), 80)
    )
}

fn full(cell: &ClientToolCell) -> String {
    format!(
        "--- 120 ---\n{}\n--- 80 ---\n{}",
        text(&cell.transcript_lines(120), 120),
        text(&cell.transcript_lines(80), 80)
    )
}

fn span_style<'a>(lines: &'a [Line<'static>], content: &str) -> &'a ratatui::text::Span<'static> {
    lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .find(|span| span.content.as_ref() == content)
        .unwrap_or_else(|| panic!("no span {content:?} in {lines:?}"))
}

#[test]
fn long_chained_command_is_one_line() {
    let cell = cell(execute(
        "c",
        ToolPhase::Completed,
        CHAINED,
        Some(("Compiling nori-tui\n   Finished release\n\n", 0)),
    ));
    assert_snapshot!(compact(&cell));
    let wide = text(&cell.display_lines(120), 120);
    let narrow = text(&cell.display_lines(40), 40);
    assert_eq!((wide.lines().count(), narrow.lines().count()), (2, 2));
    assert!(narrow.contains("+3 more"), "{narrow}");
    assert!(!wide.contains("more"), "{wide}");
    assert!(
        !wide.contains("RUST_LOG") && !wide.contains("/home/dev"),
        "{wide}"
    );
    assert!(wide.ends_with("Finished release"), "{wide}");
}

#[test]
fn pipeline_keeps_every_stage_name() {
    let cell = cell(execute(
        "p",
        ToolPhase::Completed,
        PIPELINE,
        Some(("     14 Ada\n      3 Lin", 0)),
    ));
    assert_snapshot!(compact(&cell));
}

#[test]
fn heredoc_collapses_to_line_count() {
    let cell = cell(execute(
        "h",
        ToolPhase::Completed,
        HEREDOC,
        Some(("1284 rows", 0)),
    ));
    assert_snapshot!(compact(&cell));
    let rendered = text(&cell.display_lines(120), 120);
    assert!(rendered.contains("<<heredoc (4 lines)"), "{rendered}");
    assert!(!rendered.contains("import json"), "{rendered}");
}

#[test]
fn failing_command_shows_last_output_line_in_red() {
    let cell = cell(execute(
        "f",
        ToolPhase::Failed,
        "cargo test -p parser",
        Some((
            "running 12 tests\ntest parse_empty ... FAILED\nerror: test failed, to rerun pass `-p parser --lib`",
            101,
        )),
    ));
    assert_snapshot!(compact(&cell));
    let lines = cell.display_lines(120);
    assert_eq!(lines.len(), 2);
    let output = span_style(
        &lines,
        "error: test failed, to rerun pass `-p parser --lib`",
    );
    assert_eq!(output.style.fg, Some(Color::Red));
}

#[test]
fn running_command_has_no_status_suffix() {
    let cell = cell(execute(
        "r",
        ToolPhase::InProgress,
        "npm run dev -- --port 3000",
        Some(("ready in 412 ms\n  ➜  Local:   http://localhost:3000/", 0)),
    ));
    assert_snapshot!(compact(&cell));
    let header = text(&cell.display_lines(120), 120);
    assert!(header.starts_with("• Running npm"), "{header}");
}

#[test]
fn command_names_are_bold_and_arguments_faint() {
    let lines =
        cell(execute("p", ToolPhase::Completed, PIPELINE, Some(("x", 0)))).display_lines(120);
    for name in ["git", "sort", "uniq", "head"] {
        assert!(
            span_style(&lines, name)
                .style
                .add_modifier
                .contains(Modifier::BOLD),
            "{name} is not bold"
        );
    }
    assert!(
        span_style(&lines, "…")
            .style
            .add_modifier
            .contains(Modifier::DIM)
    );
}

#[test]
fn explore_group_summarizes_and_counts_failures() {
    let group = explore_group();
    assert_snapshot!(compact(&group));
    assert_snapshot!("explore_group_full", full(&group));
    let header = text(&group.display_lines(120), 120);
    let header = header.lines().next().unwrap_or_default();
    assert!(header.contains("Explored"), "{header}");
    assert!(
        header.contains("1 read") && header.contains("3 searches"),
        "{header}"
    );
    assert!(header.contains("1 failed"), "{header}");
    let full_text = full(&group);
    assert!(
        full_text.contains("grep -rn missing_symbol src/"),
        "{full_text}"
    );
}

#[test]
fn read_only_commands_explore_and_writes_run() {
    let header = |command: &str| {
        text(
            &cell(execute("x", ToolPhase::Completed, command, Some(("", 0)))).display_lines(120),
            120,
        )
        .lines()
        .next()
        .unwrap_or_default()
        .to_string()
    };
    for command in [
        "sed -n '1,40p' src/lib.rs",
        "cat Cargo.toml | head -20",
        "cd src && rg -l Foo | head",
        "find . -name '*.rs' 2>/dev/null | sort",
        "rg x >/dev/null 2>&1",
        "rg -n Foo src | sed 's/:.*//' | sort | uniq -c",
    ] {
        assert!(header(command).contains("Explored"), "{command}");
    }
    for command in [
        "sed -i 's/a/b/' src/lib.rs",
        "rg foo > hits.txt",
        "cat a >> b",
        "rg -l Foo | xargs rm",
        "rg foo | tee hits.txt",
        "find . -name '*.tmp' -delete",
        "find . -exec rm {} \\;",
        "rg foo &> hits.txt",
        "cat a; > b",
        "ls $(rm -rf x)",
        "source ./env.sh && ls",
        "sed -ni 's/a/b/p' f",
        "sed -n '/x/w out' f",
        "sed 's/a/b/w out' f",
        "sed 's/a/b/e' f",
        "sort -o out.txt in.txt",
        "uniq in.txt out.txt",
        "fd -x rm",
        "awk '{print | \"sh\"}' f",
        "cat <<EOF > notes.md\nhi\nEOF",
        "cargo test",
    ] {
        assert!(header(command).contains("Ran"), "{command}");
    }
}

#[test]
fn edit_is_one_line_with_counts() {
    let cell = cell(ToolSnapshot {
        call_id: "e".into(),
        title: "Edit src/lib.rs".into(),
        kind: ToolKind::Edit,
        phase: ToolPhase::Completed,
        locations: vec![],
        invocation: None,
        artifacts: vec![Artifact::Diff(FileChange {
            path: cwd().join("src/lib.rs"),
            old_text: Some("fn a() {}\nfn b() {}\n".into()),
            new_text: "fn a() {}\nfn b() -> i32 { 1 }\nfn c() {}\n".into(),
        })],
        raw_input: None,
        raw_output: None,
        owner_request_id: None,
    });
    assert_snapshot!(compact(&cell));
    let rendered = text(&cell.display_lines(120), 120);
    assert_eq!(rendered.lines().count(), 1, "{rendered}");
    assert!(rendered.contains("src/lib.rs (+2 -1)"), "{rendered}");
}

#[test]
fn full_view_shows_commands_and_output_with_bold_names() {
    let chained = cell(execute(
        "c",
        ToolPhase::Completed,
        CHAINED,
        Some(("Compiling nori-tui\n   Finished release", 0)),
    ));
    let heredoc = cell(execute(
        "h",
        ToolPhase::Failed,
        HEREDOC,
        Some((
            "Traceback (most recent call last):\nFileNotFoundError: report.json",
            1,
        )),
    ));
    assert_snapshot!(format!("{}\n\n{}", full(&chained), full(&heredoc)));
    let lines = heredoc.transcript_lines(120);
    assert!(
        span_style(&lines, "python3")
            .style
            .add_modifier
            .contains(Modifier::BOLD)
    );
}

#[test]
fn full_setting_renders_full_view_inline() {
    let snapshot = execute("c", ToolPhase::Completed, CHAINED, Some(("ok", 0)));
    let full_cell = cell(snapshot.clone()).with_tool_output(ToolOutput::Full);
    assert_eq!(
        text(&full_cell.display_lines(120), 120),
        text(&cell(snapshot).transcript_lines(120), 120)
    );
}

#[test]
fn cells_without_a_setting_render_the_full_view_inline() {
    let snapshot = execute("d", ToolPhase::Completed, CHAINED, Some(("ok", 0)));
    let unconfigured = ClientToolCell::new(snapshot, cwd(), false);
    assert_eq!(
        text(&unconfigured.display_lines(120), 120),
        text(&unconfigured.transcript_lines(120), 120)
    );
}

#[test]
fn completed_command_without_output_is_one_line() {
    let cell = cell(execute(
        "n",
        ToolPhase::Completed,
        "touch build.stamp",
        Some(("\n", 0)),
    ));
    assert_eq!(text(&cell.display_lines(80), 80), "• Ran touch …");
}

#[test]
fn command_that_becomes_read_only_keeps_its_place_in_the_group() {
    let mut pending = execute("a", ToolPhase::Pending, "Terminal", None);
    pending.invocation = None;
    let mut group = cell(pending);
    group.apply_snapshot(execute(
        "a",
        ToolPhase::Completed,
        "rg -n Foo src",
        Some(("hit", 0)),
    ));
    group.merge_exploring(acp(
        "b",
        ToolKind::Read,
        "Read src/lib.rs",
        Invocation::Read {
            path: cwd().join("src/lib.rs"),
        },
    ));
    let rendered = full(&group);
    assert!(
        rendered.contains("Explored 1 read · 1 search"),
        "{rendered}"
    );
    assert!(rendered.contains("rg -n Foo src"), "{rendered}");
}

#[test]
fn group_stays_running_until_every_step_finishes() {
    let mut group = explore_group();
    group.merge_exploring(execute("exec-3", ToolPhase::InProgress, "rg -l Bar", None));
    let header = |group: &ClientToolCell| text(&group.display_lines(120), 120);
    assert!(
        header(&group).starts_with("• Exploring"),
        "{}",
        header(&group)
    );

    group.apply_snapshot(execute(
        "exec-3",
        ToolPhase::Completed,
        "rg -l Bar",
        Some(("a.rs", 0)),
    ));
    assert!(
        header(&group).starts_with("• Explored"),
        "{}",
        header(&group)
    );
}

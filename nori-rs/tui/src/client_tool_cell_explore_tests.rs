//! Exploring rows built from ACP tool-call updates, as an agent server sends
//! them, rendered the way the transcript shows them.

use std::path::PathBuf;

use insta::assert_snapshot;
use nori_config::ToolOutput;
use nori_protocol::acp::v1 as acp;
use pretty_assertions::assert_eq;
use ratatui::text::Line;
use serde_json::json;

use crate::client_tool_cell::ClientToolCell;
use crate::history_cell::HistoryCell;
use crate::presentation::Artifact;
use crate::presentation::ClientEvent;
use crate::presentation::ClientEventNormalizer;

const CWD: &str = "/home/dev/projects/nori";

fn cwd() -> PathBuf {
    PathBuf::from(CWD)
}

fn abs(rel: &str) -> String {
    format!("{CWD}/{rel}")
}

fn meta(value: serde_json::Value) -> acp::Meta {
    value.as_object().cloned().unwrap_or_default()
}

fn start(
    id: &str,
    kind: acp::ToolKind,
    title: &str,
    raw_input: Option<serde_json::Value>,
) -> acp::SessionUpdate {
    let mut call = acp::ToolCall::new(acp::ToolCallId::new(id), title)
        .kind(kind)
        .status(acp::ToolCallStatus::InProgress);
    if let Some(raw_input) = raw_input {
        call = call.raw_input(raw_input);
    }
    acp::SessionUpdate::ToolCall(call)
}

fn located(update: acp::SessionUpdate, path: &str, line: Option<u32>) -> acp::SessionUpdate {
    let acp::SessionUpdate::ToolCall(call) = update else {
        panic!("located() takes a tool call start");
    };
    acp::SessionUpdate::ToolCall(call.locations(vec![acp::ToolCallLocation::new(path).line(line)]))
}

fn completed(id: &str, raw_output: Option<serde_json::Value>) -> acp::SessionUpdate {
    let mut fields = acp::ToolCallUpdateFields::new().status(acp::ToolCallStatus::Completed);
    if let Some(raw_output) = raw_output {
        fields = fields.raw_output(raw_output);
    }
    acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(acp::ToolCallId::new(id), fields))
}

/// A metadata-only update, like the Claude adapter's post-tool hook report
/// that arrives after the call has completed.
fn meta_update(id: &str, value: serde_json::Value) -> acp::SessionUpdate {
    acp::SessionUpdate::ToolCallUpdate(
        acp::ToolCallUpdate::new(acp::ToolCallId::new(id), acp::ToolCallUpdateFields::new())
            .meta(meta(value)),
    )
}

fn claude_response(tool: &str, response: serde_json::Value) -> serde_json::Value {
    json!({ "claudeCode": { "toolName": tool, "toolResponse": response } })
}

/// Feed every update through the normalizer and into one tool cell.
fn cell_from(updates: &[acp::SessionUpdate]) -> ClientToolCell {
    let mut normalizer = ClientEventNormalizer::default();
    let mut cell: Option<ClientToolCell> = None;
    for update in updates {
        for event in normalizer.push_session_update(update) {
            let ClientEvent::ToolSnapshot(snapshot) = event else {
                continue;
            };
            match cell.as_mut() {
                Some(cell) => cell.apply_snapshot(snapshot),
                None => {
                    cell = Some(
                        ClientToolCell::new(snapshot, cwd(), false)
                            .with_tool_output(ToolOutput::Compact),
                    );
                }
            }
        }
    }
    cell.expect("updates should produce a tool cell")
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
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The single step row of a one-step exploring cell, in full view.
fn step_row(updates: &[acp::SessionUpdate]) -> String {
    let rendered = text(&cell_from(updates).transcript_lines(120), 120);
    rendered
        .lines()
        .nth(1)
        .unwrap_or_else(|| panic!("no step row in:\n{rendered}"))
        .trim()
        .to_string()
}

fn read_start(id: &str, rel: &str, input: serde_json::Value) -> acp::SessionUpdate {
    let mut input = input;
    input["file_path"] = json!(abs(rel));
    start(id, acp::ToolKind::Read, &format!("Read {rel}"), Some(input))
}

fn grep_start(id: &str, pattern: &str, dir: &str) -> acp::SessionUpdate {
    start(
        id,
        acp::ToolKind::Search,
        &format!("grep {pattern}"),
        Some(json!({ "pattern": pattern, "path": dir })),
    )
}

fn glob_start(id: &str, pattern: &str, dir: &str) -> acp::SessionUpdate {
    start(
        id,
        acp::ToolKind::Search,
        &format!("Find {pattern}"),
        Some(json!({ "pattern": pattern, "path": dir })),
    )
}

#[test]
fn read_shows_the_path_relative_to_the_working_directory() {
    let row = step_row(&[
        read_start("r", "src/session/store.rs", json!({})),
        completed("r", None),
    ]);
    assert_eq!(row, "• Read src/session/store.rs");
}

#[test]
fn read_outside_the_working_directory_keeps_its_full_path() {
    let row = step_row(&[
        start(
            "r",
            acp::ToolKind::Read,
            "Read notes.md",
            Some(json!({ "file_path": "/home/dev/projects/nori-old/notes.md" })),
        ),
        completed("r", None),
    ]);
    assert_eq!(row, "• Read /home/dev/projects/nori-old/notes.md");
}

#[test]
fn read_shows_the_requested_range_until_the_observed_range_arrives() {
    let request = read_start(
        "r",
        "src/session/store.rs",
        json!({ "offset": 40, "limit": 81 }),
    );
    let done = completed("r", Some(json!("   40\tfn load() {}")));
    assert_eq!(
        step_row(&[request.clone(), done.clone()]),
        "• Read src/session/store.rs · lines 40–120"
    );

    let observed = meta_update(
        "r",
        claude_response(
            "Read",
            json!({
                "type": "text",
                "file": {
                    "filePath": abs("src/session/store.rs"),
                    "content": "fn load() {}",
                    "startLine": 40,
                    "numLines": 81,
                    "totalLines": 845
                }
            }),
        ),
    );
    assert_eq!(
        step_row(&[request.clone(), done.clone(), observed.clone()]),
        "• Read src/session/store.rs · lines 40–120 of 845"
    );

    let later_marker = meta_update("r", json!({ "claudeCode": { "toolName": "Read" } }));
    assert_eq!(
        step_row(&[request, done, observed, later_marker]),
        "• Read src/session/store.rs · lines 40–120 of 845",
        "a later metadata update must not erase facts already received"
    );
}

#[test]
fn whole_file_read_shows_only_its_length() {
    let row = step_row(&[
        read_start("r", "Cargo.toml", json!({})),
        completed("r", None),
        meta_update(
            "r",
            claude_response(
                "Read",
                json!({ "type": "text", "file": { "startLine": 1, "numLines": 212, "totalLines": 212 } }),
            ),
        ),
    ]);
    assert_eq!(row, "• Read Cargo.toml · 212 lines");
}

#[test]
fn read_cut_short_shows_what_came_back_out_of_the_total() {
    let row = step_row(&[
        read_start("r", "docs/large.md", json!({})),
        completed("r", None),
        meta_update(
            "r",
            claude_response(
                "Read",
                json!({
                    "type": "text",
                    "file": {
                        "startLine": 1,
                        "numLines": 300,
                        "totalLines": 4000,
                        "truncatedByTokenCap": true
                    }
                }),
            ),
        ),
    ]);
    assert_eq!(row, "• Read docs/large.md · lines 1–300 of 4000");
}

#[test]
fn read_with_only_a_start_line_shows_where_it_began() {
    let row = step_row(&[
        located(
            start("r", acp::ToolKind::Read, "ReadFile", None),
            &abs("src/main.rs"),
            Some(25),
        ),
        completed("r", None),
    ]);
    assert_eq!(row, "• Read src/main.rs · from line 25");
}

#[test]
fn read_starting_at_the_first_line_shows_no_start() {
    let row = step_row(&[
        located(
            start("r", acp::ToolKind::Read, "ReadFile", None),
            &abs("src/main.rs"),
            Some(1),
        ),
        completed("r", None),
    ]);
    assert_eq!(row, "• Read src/main.rs");
}

#[test]
fn observed_range_can_arrive_before_completion() {
    let row = step_row(&[
        read_start("r", "src/main.rs", json!({ "offset": 1, "limit": 2000 })),
        meta_update(
            "r",
            claude_response(
                "Read",
                json!({ "type": "text", "file": { "startLine": 1, "numLines": 90, "totalLines": 90 } }),
            ),
        ),
        completed("r", None),
    ]);
    assert_eq!(row, "• Read src/main.rs · 90 lines");
}

#[test]
fn malformed_read_metadata_is_ignored() {
    let row = step_row(&[
        read_start(
            "r",
            "src/main.rs",
            json!({ "offset": "forty", "limit": -3 }),
        ),
        completed("r", None),
        meta_update(
            "r",
            claude_response(
                "Read",
                json!({ "type": "text", "file": { "startLine": 0, "numLines": "many", "totalLines": -1 } }),
            ),
        ),
    ]);
    assert_eq!(row, "• Read src/main.rs");
}

#[test]
fn read_line_count_from_raw_output_is_shown() {
    let row = step_row(&[
        read_start("r", "src/lib.rs", json!({})),
        completed("r", Some(json!({ "success": true, "lines": 42 }))),
    ]);
    assert_eq!(row, "• Read src/lib.rs · 42 lines");
}

/// A location at line 1 says nothing about the range, so it must not hide the
/// line count the server reported.
#[test]
fn read_line_count_from_raw_output_is_shown_for_a_read_from_the_first_line() {
    let row = step_row(&[
        located(
            start(
                "r",
                acp::ToolKind::Read,
                "Read",
                Some(json!({ "path": abs("src/lib.rs") })),
            ),
            &abs("src/lib.rs"),
            Some(1),
        ),
        completed("r", Some(json!({ "success": true, "lines": 42 }))),
    ]);
    assert_eq!(row, "• Read src/lib.rs · 42 lines");
}

#[test]
fn reported_line_count_wins_over_the_requested_range() {
    let row = step_row(&[
        read_start("r", "src/lib.rs", json!({ "offset": 40, "limit": 81 })),
        completed("r", Some(json!({ "success": true, "lines": 3 }))),
    ]);
    assert_eq!(row, "• Read src/lib.rs · 3 lines");
}

#[test]
fn out_of_range_line_numbers_are_ignored() {
    let observed = step_row(&[
        read_start("r", "src/main.rs", json!({})),
        completed("r", None),
        meta_update(
            "r",
            claude_response(
                "Read",
                json!({ "type": "text", "file": { "startLine": i64::MAX, "numLines": 2 } }),
            ),
        ),
    ]);
    assert_eq!(observed, "• Read src/main.rs");

    let requested = step_row(&[
        read_start(
            "r",
            "src/main.rs",
            json!({ "offset": i64::MAX, "limit": 5 }),
        ),
        completed("r", None),
    ]);
    assert_eq!(requested, "• Read src/main.rs");
}

#[test]
fn compact_row_keeps_the_file_name_when_the_path_is_too_long() {
    let cell = cell_from(&[
        read_start(
            "r",
            "crates/storage/src/backends/sqlite/migrations/session_store.rs",
            json!({}),
        ),
        completed("r", None),
        meta_update(
            "r",
            claude_response(
                "Read",
                json!({ "type": "text", "file": { "startLine": 1, "numLines": 88, "totalLines": 88 } }),
            ),
        ),
    ]);
    let rendered = text(&cell.display_lines(48), 48);
    let row = rendered
        .lines()
        .nth(1)
        .unwrap_or_default()
        .trim()
        .to_string();
    assert_eq!(row, "└ Read session_store.rs · 88 lines");
}

#[test]
fn read_line_count_from_raw_output_stays_the_tool_output_text() {
    let mut normalizer = ClientEventNormalizer::default();
    let artifacts = [
        read_start("r", "src/lib.rs", json!({})),
        completed("r", Some(json!({ "success": true, "lines": 42 }))),
    ]
    .iter()
    .flat_map(|update| normalizer.push_session_update(update))
    .filter_map(|event| match event {
        ClientEvent::ToolSnapshot(snapshot) => Some(snapshot.artifacts),
        _ => None,
    })
    .last()
    .unwrap_or_default();
    assert_eq!(
        artifacts,
        vec![Artifact::Text {
            text: "Read 42 lines".into()
        }]
    );
}

#[test]
fn search_counts_matches_and_files_when_the_server_reports_them() {
    let row = step_row(&[
        grep_start("s", "TODO", &abs("src")),
        completed("s", None),
        meta_update(
            "s",
            claude_response(
                "Grep",
                json!({ "mode": "count", "numFiles": 4, "numMatches": 12, "filenames": [] }),
            ),
        ),
    ]);
    assert_eq!(row, "• Search TODO in src · 12 matches in 4 files");
}

#[test]
fn search_counts_a_single_file_in_the_singular() {
    let one = step_row(&[
        grep_start("s", "fn main", &abs("src")),
        completed("s", None),
        meta_update(
            "s",
            claude_response(
                "Grep",
                json!({ "mode": "files_with_matches", "numFiles": 1, "filenames": ["src/main.rs"] }),
            ),
        ),
    ]);
    assert_eq!(one, "• Search fn main in src · 1 file");
}

#[test]
fn file_list_search_without_a_total_marks_its_limited_count() {
    let row = step_row(&[
        grep_start("s", "use crate", &abs("src")),
        completed("s", None),
        meta_update(
            "s",
            claude_response(
                "Grep",
                json!({ "mode": "files_with_matches", "numFiles": 250, "appliedLimit": 250, "filenames": [] }),
            ),
        ),
    ]);
    assert_eq!(row, "• Search use crate in src · 250+ files");
}

#[test]
fn search_with_no_hits_says_so() {
    let row = step_row(&[
        grep_start("s", "missing_symbol", &abs("src")),
        completed("s", None),
        meta_update(
            "s",
            claude_response(
                "Grep",
                json!({ "mode": "files_with_matches", "numFiles": 0, "filenames": [] }),
            ),
        ),
    ]);
    assert_eq!(row, "• Search missing_symbol in src · no matches");
}

#[test]
fn file_search_prefers_the_complete_total_over_the_shown_list() {
    let complete = step_row(&[
        glob_start("g", "**/*.rs", &abs("tui")),
        completed("g", None),
        meta_update(
            "g",
            claude_response(
                "Glob",
                json!({ "numFiles": 100, "truncated": true, "totalMatches": 437, "countIsComplete": true, "filenames": [] }),
            ),
        ),
    ]);
    assert_eq!(complete, "• Search **/*.rs in tui · 437 files");
}

#[test]
fn file_search_cut_short_marks_its_count() {
    let partial = step_row(&[
        glob_start("g", "**/*.rs", &abs("tui")),
        completed("g", None),
        meta_update(
            "g",
            claude_response(
                "Glob",
                json!({ "numFiles": 100, "truncated": true, "filenames": [] }),
            ),
        ),
    ]);
    assert_eq!(partial, "• Search **/*.rs in tui · 100+ files");
}

#[test]
fn search_in_the_working_directory_omits_the_folder() {
    let row = step_row(&[grep_start("s", "TODO", CWD), completed("s", None)]);
    assert_eq!(row, "• Search TODO");
}

#[test]
fn search_counts_from_opencode_style_raw_output_metadata() {
    let row = step_row(&[
        grep_start("s", "TODO", &abs("src")),
        completed(
            "s",
            Some(json!({ "metadata": { "matches": 7, "truncated": false } })),
        ),
    ]);
    assert_eq!(row, "• Search TODO in src · 7 matches");
}

fn grep_row(pattern: &str, response: serde_json::Value) -> String {
    step_row(&[
        grep_start("s", pattern, &abs("src")),
        completed("s", None),
        meta_update("s", claude_response("Grep", response)),
    ])
}

fn glob_row(response: serde_json::Value) -> String {
    step_row(&[
        glob_start("g", "**/*.rs", &abs("tui")),
        completed("g", None),
        meta_update("g", claude_response("Glob", response)),
    ])
}

/// Claude Code's content-mode Grep reports `numFiles: 0` and no match count
/// even when it found hits; `numLines`/`totalLines` count output lines,
/// including context lines, not hits.
#[test]
fn content_mode_search_with_hits_shows_no_count() {
    let row = grep_row(
        "TODO",
        json!({
            "mode": "content",
            "numFiles": 0,
            "filenames": [],
            "content": "src/a.rs:3:// TODO tidy\nsrc/b.rs:9:// TODO split",
            "numLines": 2,
            "totalLines": 2
        }),
    );
    assert_eq!(row, "• Search TODO in src");
}

#[test]
fn content_mode_search_never_reports_output_lines_as_hits() {
    let row = grep_row(
        "fn render",
        json!({
            "mode": "content",
            "numFiles": 0,
            "filenames": [],
            "content": "src/a.rs-1-use x;\nsrc/a.rs:2:fn render() {}",
            "numLines": 20,
            "totalLines": 57,
            "appliedLimit": 20
        }),
    );
    assert_eq!(row, "• Search fn render in src");
}

#[test]
fn content_mode_search_with_no_output_says_no_matches() {
    let row = grep_row(
        "missing_symbol",
        json!({
            "mode": "content",
            "numFiles": 0,
            "filenames": [],
            "content": "",
            "numLines": 0,
            "totalLines": 0
        }),
    );
    assert_eq!(row, "• Search missing_symbol in src · no matches");
}

#[test]
fn content_mode_search_without_a_mode_field_shows_no_count() {
    let row = grep_row(
        "TODO",
        json!({ "numFiles": 0, "filenames": [], "content": "src/a.rs:3:TODO", "numLines": 1 }),
    );
    assert_eq!(row, "• Search TODO in src");
}

#[test]
fn file_list_search_cut_off_by_a_limit_shows_the_exact_total() {
    let row = grep_row(
        "use crate",
        json!({
            "mode": "files_with_matches",
            "numFiles": 250,
            "totalFiles": 1203,
            "appliedLimit": 250,
            "filenames": []
        }),
    );
    assert_eq!(row, "• Search use crate in src · 1203 files");
}

#[test]
fn count_mode_totals_are_exact_when_a_limit_trims_the_list() {
    let row = grep_row(
        "unwrap",
        json!({
            "mode": "count",
            "numFiles": 60,
            "numMatches": 412,
            "appliedLimit": 20,
            "content": "",
            "filenames": []
        }),
    );
    assert_eq!(row, "• Search unwrap in src · 412 matches in 60 files");
}

#[test]
fn timed_out_count_mode_search_marks_both_counts_as_lower_bounds() {
    let row = grep_row(
        "unwrap",
        json!({
            "mode": "count",
            "numFiles": 60,
            "numMatches": 412,
            "timedOut": true,
            "content": "",
            "filenames": []
        }),
    );
    assert_eq!(row, "• Search unwrap in src · 412+ matches in 60+ files");
}

#[test]
fn timed_out_file_list_search_marks_its_count() {
    let row = grep_row(
        "use crate",
        json!({
            "mode": "files_with_matches",
            "numFiles": 37,
            "totalFiles": 37,
            "timedOut": true,
            "filenames": []
        }),
    );
    assert_eq!(row, "• Search use crate in src · 37+ files");
}

#[test]
fn timed_out_search_with_no_hits_is_not_called_empty() {
    let files = grep_row(
        "missing_symbol",
        json!({
            "mode": "files_with_matches",
            "numFiles": 0,
            "totalFiles": 0,
            "timedOut": true,
            "filenames": []
        }),
    );
    assert_eq!(
        files,
        "• Search missing_symbol in src · no matches (incomplete)"
    );
    let content = grep_row(
        "missing_symbol",
        json!({
            "mode": "content",
            "numFiles": 0,
            "filenames": [],
            "content": "",
            "numLines": 0,
            "totalLines": 0,
            "timedOut": true
        }),
    );
    assert_eq!(
        content,
        "• Search missing_symbol in src · no matches (incomplete)"
    );
}

#[test]
fn timed_out_file_search_marks_its_count() {
    let partial = glob_row(json!({
        "numFiles": 12,
        "truncated": false,
        "totalMatches": 12,
        "countIsComplete": false,
        "timedOut": true,
        "filenames": []
    }));
    assert_eq!(partial, "• Search **/*.rs in tui · 12+ files");
}

#[test]
fn timed_out_file_search_with_no_hits_is_not_called_empty() {
    let empty = glob_row(json!({
        "numFiles": 0,
        "truncated": false,
        "totalMatches": 0,
        "countIsComplete": false,
        "timedOut": true,
        "filenames": []
    }));
    assert_eq!(empty, "• Search **/*.rs in tui · no matches (incomplete)");
    let legacy =
        glob_row(json!({ "numFiles": 0, "truncated": false, "timedOut": true, "filenames": [] }));
    assert_eq!(legacy, "• Search **/*.rs in tui · no matches (incomplete)");
}

#[test]
fn cut_off_search_with_no_hits_from_raw_output_metadata_is_not_called_empty() {
    let row = step_row(&[
        grep_start("s", "TODO", &abs("src")),
        completed(
            "s",
            Some(json!({ "metadata": { "matches": 0, "truncated": true } })),
        ),
    ]);
    assert_eq!(row, "• Search TODO in src · no matches (incomplete)");
}

#[test]
fn running_search_shows_no_counts_yet() {
    let updates = [grep_start("s", "TODO", &abs("src"))];
    let rendered = text(&cell_from(&updates).transcript_lines(120), 120);
    assert!(rendered.contains("Exploring"), "{rendered}");
    assert_eq!(step_row(&updates), "• Search TODO in src");
}

#[test]
fn search_without_counts_shows_no_details() {
    let row = step_row(&[
        grep_start("s", "TODO", &abs("src")),
        completed("s", Some(json!("src/a.rs:1: TODO\nsrc/b.rs:9: TODO"))),
    ]);
    assert_eq!(row, "• Search TODO in src");
}

fn detailed_group() -> ClientToolCell {
    let mut normalizer = ClientEventNormalizer::default();
    let mut snapshots = Vec::new();
    let mut push = |update: acp::SessionUpdate| {
        for event in normalizer.push_session_update(&update) {
            if let ClientEvent::ToolSnapshot(snapshot) = event {
                snapshots.push(snapshot);
            }
        }
    };
    push(read_start(
        "r1",
        "nori-rs/tui/src/client_tool_cell.rs",
        json!({ "offset": 180, "limit": 80 }),
    ));
    push(completed("r1", None));
    push(meta_update(
        "r1",
        claude_response(
            "Read",
            json!({ "type": "text", "file": { "startLine": 180, "numLines": 80, "totalLines": 2230 } }),
        ),
    ));
    push(read_start("r2", "Cargo.toml", json!({})));
    push(completed("r2", None));
    push(meta_update(
        "r2",
        claude_response(
            "Read",
            json!({ "type": "text", "file": { "startLine": 1, "numLines": 64, "totalLines": 64 } }),
        ),
    ));
    push(grep_start("s1", "render_exploring_lines", &abs("nori-rs")));
    push(completed("s1", None));
    push(meta_update(
        "s1",
        claude_response(
            "Grep",
            json!({ "mode": "count", "numFiles": 2, "numMatches": 3, "filenames": [] }),
        ),
    ));
    push(glob_start("g1", "**/*.snap", &abs("nori-rs/tui")));
    push(completed("g1", None));
    push(meta_update(
        "g1",
        claude_response(
            "Glob",
            json!({ "numFiles": 41, "truncated": false, "filenames": [] }),
        ),
    ));

    push(grep_start("s2", "TODO", &abs("nori-rs/tui/src")));
    push(completed("s2", None));
    push(meta_update(
        "s2",
        claude_response(
            "Grep",
            json!({ "mode": "content", "numFiles": 0, "filenames": [], "content": "a.rs:1:// TODO", "numLines": 1, "totalLines": 1 }),
        ),
    ));
    push(glob_start("g2", "**/*.orig", &abs("nori-rs")));
    push(completed("g2", None));
    push(meta_update(
        "g2",
        claude_response(
            "Glob",
            json!({ "numFiles": 0, "truncated": false, "totalMatches": 0, "countIsComplete": false, "timedOut": true, "filenames": [] }),
        ),
    ));

    let mut snapshots = snapshots.into_iter();
    let mut cell = ClientToolCell::new(snapshots.next().expect("first snapshot"), cwd(), false)
        .with_tool_output(ToolOutput::Compact);
    cell.mark_exploring();
    for snapshot in snapshots {
        if cell.holds_call(&snapshot.call_id) {
            cell.apply_snapshot(snapshot);
        } else {
            cell.merge_exploring(snapshot);
        }
    }
    cell
}

#[test]
fn explore_group_with_details() {
    let group = detailed_group();
    assert_snapshot!(format!(
        "--- full 120 ---\n{}\n--- full 60 ---\n{}\n--- compact 120 ---\n{}\n--- compact 40 ---\n{}",
        text(&group.transcript_lines(120), 120),
        text(&group.transcript_lines(60), 60),
        text(&group.display_lines(120), 120),
        text(&group.display_lines(40), 40),
    ));
}

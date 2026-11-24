//! Line ranges and hit counts for read and search tool calls.
//!
//! Every value comes from structured fields the agent server sent: the tool
//! input, the tool output, or the Claude adapter's `_meta.claudeCode`
//! report. Nothing is parsed out of output text and no file is opened, so a
//! server that sends none of these fields gets no details. Values the server
//! observed (what a read returned) win over values the agent requested.

use nori_protocol::acp::v1 as acp;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ToolDetails {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines: Option<LineSpan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hits: Option<HitCount>,
}

/// The lines a read covered. `total` is the line count the server reported
/// for the file, or for the read when it gave no position.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LineSpan {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<i64>,
}

/// What a search found. `more` marks a count that is only a lower bound,
/// because the server cut the search off at a limit or a time limit, so the
/// real numbers are at least the ones shown.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct HitCount {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matches: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<i64>,
    #[serde(default)]
    pub more: bool,
}

impl ToolDetails {
    pub fn is_empty(&self) -> bool {
        self.lines.is_none() && self.hits.is_none()
    }

    /// A short phrase such as `lines 40–120 of 845` or `12 matches in 4 files`.
    pub fn summary(&self) -> Option<String> {
        if let Some(hits) = &self.hits {
            return hits.summary();
        }
        self.lines.as_ref().and_then(LineSpan::summary)
    }
}

impl LineSpan {
    fn summary(&self) -> Option<String> {
        match (self.start, self.end, self.total) {
            (Some(1), Some(end), Some(total)) if end == total => Some(lines(total)),
            (Some(start), Some(end), Some(total)) => {
                Some(format!("lines {start}–{end} of {total}"))
            }
            (Some(start), Some(end), None) => Some(format!("lines {start}–{end}")),
            (Some(start), None, _) if start > 1 => Some(format!("from line {start}")),
            (_, _, Some(total)) => Some(lines(total)),
            (Some(_), None, None) | (None, _, None) => None,
        }
    }
}

impl HitCount {
    fn summary(&self) -> Option<String> {
        let more = if self.more { "+" } else { "" };
        match (self.matches, self.files) {
            (Some(0), _) | (None, Some(0)) if self.more => {
                Some("no matches (incomplete)".to_string())
            }
            (Some(0), _) | (None, Some(0)) => Some("no matches".to_string()),
            (Some(matches), Some(files)) => Some(format!(
                "{matches}{more} {} in {files}{more} {}",
                plural(matches, "match", "matches"),
                plural(files, "file", "files")
            )),
            (Some(matches), None) => Some(format!(
                "{matches}{more} {}",
                plural(matches, "match", "matches")
            )),
            (None, Some(files)) => {
                Some(format!("{files}{more} {}", plural(files, "file", "files")))
            }
            (None, None) => None,
        }
    }
}

fn lines(count: i64) -> String {
    format!("{count} {}", plural(count, "line", "lines"))
}

fn plural(count: i64, one: &'static str, many: &'static str) -> &'static str {
    if count == 1 { one } else { many }
}

pub(super) fn details_from_tool_call(tool_call: &acp::ToolCall) -> ToolDetails {
    // The Claude adapter's report: `_meta.claudeCode` with `toolName` and the
    // Claude Code `toolResponse`.
    let report = tool_call
        .meta
        .as_ref()
        .and_then(|meta| meta.get("claudeCode"))
        .and_then(|claude| {
            let name = claude.get("toolName")?.as_str()?;
            Some((name, claude.get("toolResponse")?))
        });
    let reported_lines = || {
        count(tool_call.raw_output.as_ref()?.get("lines")).map(|total| LineSpan {
            start: None,
            end: None,
            total: Some(total),
        })
    };
    match tool_call.kind {
        acp::ToolKind::Read => ToolDetails {
            lines: observed_read(report)
                .or_else(reported_lines)
                .or_else(|| requested_read(tool_call)),
            hits: None,
        },
        acp::ToolKind::Search => ToolDetails {
            lines: None,
            hits: claude_search_hits(report).or_else(|| raw_output_search_hits(tool_call)),
        },
        acp::ToolKind::Edit
        | acp::ToolKind::Delete
        | acp::ToolKind::Move
        | acp::ToolKind::Execute
        | acp::ToolKind::Think
        | acp::ToolKind::Fetch
        | acp::ToolKind::SwitchMode
        | acp::ToolKind::Other => ToolDetails::default(),
        _ => ToolDetails::default(),
    }
}

/// Deep-merge an update's `_meta` into the stored call, so a report sent in
/// one update survives later updates that carry other metadata.
pub(super) fn merge_meta(target: &mut Option<acp::Meta>, update: &acp::Meta) {
    let target = target.get_or_insert_with(acp::Meta::new);
    for (key, value) in update {
        match (target.get_mut(key), value) {
            (Some(Value::Object(existing)), Value::Object(incoming)) => {
                let mut nested = Some(std::mem::take(existing));
                merge_meta(&mut nested, incoming);
                *existing = nested.unwrap_or_default();
            }
            _ => {
                target.insert(key.clone(), value.clone());
            }
        }
    }
}

fn count(value: Option<&Value>) -> Option<i64> {
    value?.as_i64().filter(|count| *count >= 0)
}

fn line_number(value: Option<&Value>) -> Option<i64> {
    value?.as_i64().filter(|line| *line >= 1)
}

/// The last line of `count` lines starting at `start`, or `None` when the
/// numbers are too large to be real.
fn last_line(start: i64, count: i64) -> Option<i64> {
    start.checked_add(count)?.checked_sub(1)
}

fn observed_read(report: Option<(&str, &Value)>) -> Option<LineSpan> {
    let ("Read", response) = report? else {
        return None;
    };
    let file = response.get("file")?;
    let total = count(file.get("totalLines"));
    let start = line_number(file.get("startLine"));
    let returned = count(file.get("numLines"));
    let (start, end) = match (start, returned) {
        (Some(start), Some(returned)) if returned > 0 => match last_line(start, returned) {
            Some(end) => (Some(start), Some(end)),
            None => return None,
        },
        _ => (None, None),
    };
    (start.is_some() || total.is_some()).then_some(LineSpan { start, end, total })
}

fn requested_read(tool_call: &acp::ToolCall) -> Option<LineSpan> {
    let input = tool_call.raw_input.as_ref();
    let field = |key: &str| input.and_then(|input| input.get(key));
    let span = match (
        line_number(field("offset")),
        count(field("limit")).filter(|limit| *limit > 0),
        line_number(field("start_line")),
        line_number(field("end_line")),
    ) {
        (_, _, Some(start), Some(end)) if end >= start => (start, Some(end)),
        (_, _, Some(start), _) => (start, None),
        (Some(offset), Some(limit), None, _) => (offset, Some(last_line(offset, limit)?)),
        (None, Some(limit), None, _) => (1, Some(limit)),
        (Some(offset), None, None, _) => (offset, None),
        (None, None, None, _) => {
            let line = tool_call.locations.first()?.line?;
            (i64::from(line), None)
        }
    };
    match span {
        (start, None) if start <= 1 => None,
        (start, end) => Some(LineSpan {
            start: Some(start),
            end,
            total: None,
        }),
    }
}

fn claude_search_hits(report: Option<(&str, &Value)>) -> Option<HitCount> {
    let (name, response) = report?;
    let timed_out = response.get("timedOut").and_then(Value::as_bool) == Some(true);
    match name {
        "Grep" => claude_grep_hits(response, timed_out),
        "Glob" => {
            let (files, more) = match count(response.get("totalMatches")) {
                Some(total) => (
                    Some(total),
                    response.get("countIsComplete").and_then(Value::as_bool) == Some(false),
                ),
                None => (
                    count(response.get("numFiles")),
                    response.get("truncated").and_then(Value::as_bool) == Some(true),
                ),
            };
            files.map(|files| HitCount {
                matches: None,
                files: Some(files),
                more: more || timed_out,
            })
        }
        _ => None,
    }
}

/// Claude Code's Grep report depends on its output mode:
/// - `content`: `numFiles` is always 0 and there is no match count;
///   `numLines`/`totalLines` count output lines, including context lines.
///   Only an empty output (`totalLines: 0`) says anything about hits.
/// - `count`: `numMatches` and `numFiles` are totals over every result;
///   `appliedLimit` trims only the listed lines.
/// - `files_with_matches`: `totalFiles` is the total before `appliedLimit`
///   and `numFiles` is the number listed after it.
fn claude_grep_hits(response: &Value, timed_out: bool) -> Option<HitCount> {
    let mode = response.get("mode").and_then(Value::as_str);
    let has = |key: &str| response.get(key).is_some();
    let mode = mode.unwrap_or(if has("numMatches") {
        "count"
    } else if has("numLines") || has("totalLines") {
        "content"
    } else {
        "files_with_matches"
    });
    let hits = match mode {
        "content" => {
            let empty = count(response.get("totalLines")) == Some(0);
            return empty.then_some(HitCount {
                matches: Some(0),
                files: None,
                more: timed_out,
            });
        }
        "count" => HitCount {
            matches: count(response.get("numMatches")),
            files: count(response.get("numFiles")),
            more: timed_out,
        },
        "files_with_matches" => match count(response.get("totalFiles")) {
            Some(total) => HitCount {
                matches: None,
                files: Some(total),
                more: timed_out,
            },
            None => HitCount {
                matches: None,
                files: count(response.get("numFiles")),
                more: timed_out || count(response.get("appliedLimit")).is_some(),
            },
        },
        _ => return None,
    };
    (hits.matches.is_some() || hits.files.is_some()).then_some(hits)
}

/// Counts under `rawOutput.metadata`, as OpenCode reports its grep (`matches`)
/// and glob (`count`, a file count) results.
fn raw_output_search_hits(tool_call: &acp::ToolCall) -> Option<HitCount> {
    let metadata = tool_call.raw_output.as_ref()?.get("metadata")?;
    let matches = count(metadata.get("matches"));
    let files = count(metadata.get("count"));
    (matches.is_some() || files.is_some()).then(|| HitCount {
        matches,
        files,
        more: metadata.get("truncated").and_then(Value::as_bool) == Some(true),
    })
}

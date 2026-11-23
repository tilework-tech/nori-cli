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
    pub start: Option<i64>,
    pub end: Option<i64>,
    pub total: Option<i64>,
}

/// What a search found. `more` marks a count the server cut off at a limit,
/// so the real number is at least the one shown.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct HitCount {
    pub matches: Option<i64>,
    pub files: Option<i64>,
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
            (Some(0), _) | (None, Some(0)) => Some("no matches".to_string()),
            (Some(matches), Some(files)) => Some(format!(
                "{matches}{more} {} in {files} {}",
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
    let report = claude_report(tool_call);
    match tool_call.kind {
        acp::ToolKind::Read => ToolDetails {
            lines: observed_read(report)
                .or_else(|| requested_read(tool_call))
                .or_else(|| raw_output_line_count(tool_call)),
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

/// The Claude adapter's tool report: `_meta.claudeCode` with `toolName` and
/// the Claude Code `toolResponse`.
fn claude_report(tool_call: &acp::ToolCall) -> Option<(&str, &Value)> {
    let claude = tool_call.meta.as_ref()?.get("claudeCode")?;
    let name = claude.get("toolName")?.as_str()?;
    let response = claude.get("toolResponse")?;
    Some((name, response))
}

fn count(value: Option<&Value>) -> Option<i64> {
    value?.as_i64().filter(|count| *count >= 0)
}

fn line_number(value: Option<&Value>) -> Option<i64> {
    value?.as_i64().filter(|line| *line >= 1)
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
        (Some(start), Some(returned)) if returned > 0 => (Some(start), Some(start + returned - 1)),
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
        (Some(offset), Some(limit), None, _) => (offset, Some(offset + limit - 1)),
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

fn raw_output_line_count(tool_call: &acp::ToolCall) -> Option<LineSpan> {
    let total = count(tool_call.raw_output.as_ref()?.get("lines"))?;
    Some(LineSpan {
        start: None,
        end: None,
        total: Some(total),
    })
}

fn claude_search_hits(report: Option<(&str, &Value)>) -> Option<HitCount> {
    let (name, response) = report?;
    match name {
        "Grep" => {
            let matches = count(response.get("numMatches"));
            let files = count(response.get("numFiles"));
            (matches.is_some() || files.is_some()).then(|| HitCount {
                matches,
                files,
                more: count(response.get("appliedLimit")).is_some(),
            })
        }
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
                more,
            })
        }
        _ => None,
    }
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

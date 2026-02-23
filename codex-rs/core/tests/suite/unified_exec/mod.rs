#![cfg(not(target_os = "windows"))]
use std::collections::HashMap;
use std::sync::OnceLock;

use anyhow::Context;
use anyhow::Result;
use codex_core::features::Feature;
use codex_core::protocol::AskForApproval;
use codex_core::protocol::EventMsg;
use codex_core::protocol::ExecCommandSource;
use codex_core::protocol::Op;
use codex_core::protocol::SandboxPolicy;
use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::user_input::UserInput;
use core_test_support::assert_regex_match;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::skip_if_sandbox;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use core_test_support::wait_for_event_match;
use core_test_support::wait_for_event_with_timeout;
use regex_lite::Regex;
use serde_json::Value;
use serde_json::json;
use tokio::time::Duration;

fn extract_output_text(item: &Value) -> Option<&str> {
    item.get("output").and_then(|value| match value {
        Value::String(text) => Some(text.as_str()),
        Value::Object(obj) => obj.get("content").and_then(Value::as_str),
        _ => None,
    })
}

#[derive(Debug)]
struct ParsedUnifiedExecOutput {
    chunk_id: Option<String>,
    wall_time_seconds: f64,
    process_id: Option<String>,
    exit_code: Option<i32>,
    original_token_count: Option<usize>,
    output: String,
}

#[allow(clippy::expect_used)]
fn parse_unified_exec_output(raw: &str) -> Result<ParsedUnifiedExecOutput> {
    static OUTPUT_REGEX: OnceLock<Regex> = OnceLock::new();
    let regex = OUTPUT_REGEX.get_or_init(|| {
        Regex::new(concat!(
            r#"(?s)^(?:Total output lines: \d+\n\n)?"#,
            r#"(?:Chunk ID: (?P<chunk_id>[^\n]+)\n)?"#,
            r#"Wall time: (?P<wall_time>-?\d+(?:\.\d+)?) seconds\n"#,
            r#"(?:Process exited with code (?P<exit_code>-?\d+)\n)?"#,
            r#"(?:Process running with session ID (?P<process_id>-?\d+)\n)?"#,
            r#"(?:Original token count: (?P<original_token_count>\d+)\n)?"#,
            r#"Output:\n?(?P<output>.*)$"#,
        ))
        .expect("valid unified exec output regex")
    });

    let cleaned = raw.trim_matches('\r');
    let captures = regex
        .captures(cleaned)
        .ok_or_else(|| anyhow::anyhow!("missing Output section in unified exec output {raw}"))?;

    let chunk_id = captures
        .name("chunk_id")
        .map(|value| value.as_str().to_string());

    let wall_time_seconds = captures
        .name("wall_time")
        .expect("wall_time group present")
        .as_str()
        .parse::<f64>()
        .context("failed to parse wall time seconds")?;

    let exit_code = captures
        .name("exit_code")
        .map(|value| {
            value
                .as_str()
                .parse::<i32>()
                .context("failed to parse exit code from unified exec output")
        })
        .transpose()?;

    let process_id = captures
        .name("process_id")
        .map(|value| value.as_str().to_string());

    let original_token_count = captures
        .name("original_token_count")
        .map(|value| {
            value
                .as_str()
                .parse::<usize>()
                .context("failed to parse original token count from unified exec output")
        })
        .transpose()?;

    let output = captures
        .name("output")
        .expect("output group present")
        .as_str()
        .to_string();

    Ok(ParsedUnifiedExecOutput {
        chunk_id,
        wall_time_seconds,
        process_id,
        exit_code,
        original_token_count,
        output,
    })
}

fn collect_tool_outputs(bodies: &[Value]) -> Result<HashMap<String, ParsedUnifiedExecOutput>> {
    let mut outputs = HashMap::new();
    for body in bodies {
        if let Some(items) = body.get("input").and_then(Value::as_array) {
            for item in items {
                if item.get("type").and_then(Value::as_str) != Some("function_call_output") {
                    continue;
                }
                if let Some(call_id) = item.get("call_id").and_then(Value::as_str) {
                    let content = extract_output_text(item)
                        .ok_or_else(|| anyhow::anyhow!("missing tool output content"))?;
                    let trimmed = content.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let parsed = parse_unified_exec_output(content).with_context(|| {
                        format!("failed to parse unified exec output for {call_id}")
                    })?;
                    outputs.insert(call_id.to_string(), parsed);
                }
            }
        }
    }
    Ok(outputs)
}

mod part1;
mod part2;
mod part3;
mod part4;

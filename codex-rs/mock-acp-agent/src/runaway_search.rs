use agent_client_protocol::{self as acp};
use serde_json::json;
use tokio::time::Duration;
use tokio::time::sleep;

use crate::MockAgent;

const RUNAWAY_TITLE: &str = "Search runaway-pattern in runaway-search-fixture";
const RUNAWAY_CALL_ID: &str = "runaway-search-001";

pub(crate) async fn run(
    agent: &MockAgent,
    session_id: acp::SessionId,
) -> Result<acp::PromptResponse, acp::Error> {
    let updates = env_usize("MOCK_AGENT_RUNAWAY_SEARCH_UPDATES", 60);
    let lines_per_update = env_usize("MOCK_AGENT_RUNAWAY_SEARCH_LINES_PER_UPDATE", 25);
    let line_len = env_usize("MOCK_AGENT_RUNAWAY_SEARCH_LINE_LEN", 96);
    let delay_ms = env_u64("MOCK_AGENT_RUNAWAY_SEARCH_DELAY_MS", 2);
    let skip_completion = std::env::var("MOCK_AGENT_RUNAWAY_SEARCH_SKIP_COMPLETION").is_ok();
    let skip_final_text = std::env::var("MOCK_AGENT_RUNAWAY_SEARCH_SKIP_FINAL_TEXT").is_ok();

    eprintln!(
        "Mock agent: sending runaway search stream updates={updates} lines_per_update={lines_per_update} line_len={line_len} delay_ms={delay_ms}"
    );

    let call_id = acp::ToolCallId::new(RUNAWAY_CALL_ID);
    agent
        .send_tool_call(
            session_id.clone(),
            acp::ToolCall::new(call_id.clone(), RUNAWAY_TITLE)
                .kind(acp::ToolKind::Search)
                .status(acp::ToolCallStatus::Pending)
                .raw_input(json!({
                    "pattern": "runaway-pattern",
                    "path": "runaway-search-fixture",
                })),
        )
        .await?;

    let padding = "x".repeat(line_len);
    let mut cumulative_output = String::new();

    for update_index in 0..updates {
        if agent.cancel_requested.get() {
            return Ok(acp::PromptResponse::new(acp::StopReason::Cancelled));
        }

        for line_index in 0..lines_per_update {
            let prefix = format!(
                "/repo/runaway-search-fixture/src/path_{update_index:04}_{line_index:04}.rs:{}: runaway-pattern ",
                update_index * lines_per_update + line_index + 1
            );
            cumulative_output.push_str(&prefix);
            let padding_len = line_len.saturating_sub(prefix.len());
            cumulative_output.push_str(&padding[..padding_len]);
            cumulative_output.push('\n');
        }

        agent
            .send_tool_call_update(
                session_id.clone(),
                acp::ToolCallUpdate::new(
                    call_id.clone(),
                    acp::ToolCallUpdateFields::new()
                        .status(acp::ToolCallStatus::InProgress)
                        .content(vec![acp::ToolCallContent::Content(acp::Content::new(
                            acp::ContentBlock::Text(acp::TextContent::new(
                                cumulative_output.clone(),
                            )),
                        ))]),
                ),
            )
            .await?;

        if delay_ms > 0 {
            sleep(Duration::from_millis(delay_ms)).await;
        }
    }

    if !skip_completion {
        agent
            .send_tool_call_update(
                session_id.clone(),
                acp::ToolCallUpdate::new(
                    call_id,
                    acp::ToolCallUpdateFields::new().status(acp::ToolCallStatus::Completed),
                ),
            )
            .await?;
    }

    if !skip_final_text {
        agent
            .send_text_chunk(session_id, "Runaway search scenario complete.")
            .await?;
    }

    Ok(acp::PromptResponse::new(acp::StopReason::EndTurn))
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
}

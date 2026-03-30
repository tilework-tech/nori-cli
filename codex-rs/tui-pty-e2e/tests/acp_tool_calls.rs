//! E2E tests for ACP tool call rendering in the TUI
//!
//! These tests verify that tool calls from ACP agents are properly
//! rendered in the TUI using the ACP-native tool cell path.
//!
//! ## Test Strategy
//!
//! The tests configure the mock-acp-agent to emit ToolCall/ToolCallUpdate
//! events, then verify the TUI displays them correctly. This validates
//! the entire ACP-to-TUI flow:
//!
//! 1. Mock agent sends `SessionUpdate::ToolCall` / `ToolCallUpdate`
//! 2. ACP backend normalizes them into `ClientEvent::ToolSnapshot`
//! 3. TUI chatwidget renders via `ClientToolCell`
//!
//! ## Expected TUI Output Format
//!
//! Active tool calls display as:
//! ```text
//! • Tool [in progress]: Reading configuration file (read)
//!   └ Read: /etc/config.toml
//! ```
//!
//! Completed tool calls display as:
//! ```text
//! • Tool [completed]: Reading configuration file (read)
//!   └ Read: /etc/config.toml
//!     Output: Configuration loaded successfully
//! ```

use std::time::Duration;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TIMEOUT_PRESNAPSHOT;
use tui_pty_e2e::TuiSession;
use tui_pty_e2e::normalize_for_input_snapshot;

/// Test that an ACP tool call is rendered in the TUI
///
/// This test verifies the full ACP tool call rendering pipeline:
/// 1. Mock agent emits a ToolCall event
/// 2. ACP normalizes it to `ClientEvent::ToolSnapshot`
/// 3. TUI displays it using `ClientToolCell`
///
/// ## Prerequisites for this test to pass:
/// - Mock agent must support MOCK_AGENT_TOOL_CALL env var
/// - ACP normalization must emit `ClientEvent::ToolSnapshot`
/// - TUI must render the native ACP tool cell path
#[test]
#[cfg(target_os = "linux")]
fn test_acp_tool_call_rendered_in_tui() {
    // Configure mock agent to send a tool call
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        // Configure mock agent to emit a tool call before responding
        // The mock agent checks MOCK_AGENT_SEND_TOOL_CALL (not MOCK_AGENT_TOOL_CALL)
        .with_agent_env("MOCK_AGENT_SEND_TOOL_CALL", "1");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn in ACP mode");

    // Wait for startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Send a prompt that triggers the tool call
    session.send_str("Read a file for me").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for the native ACP tool cell to appear in the TUI.
    let tool_call_appeared = session.wait_for(
        |screen| screen.contains("Tool [") && screen.contains("Reading configuration file"),
        Duration::from_secs(10),
    );

    match tool_call_appeared {
        Ok(()) => {
            // Tool call UI appeared
            let contents = session.screen_contents();

            assert!(
                (contents.contains("Tool [pending]: Reading configuration file (read)")
                    || contents.contains("Tool [completed]: Reading configuration file (read)"))
                    && contents.contains("Read: /etc/config.toml"),
                "Tool call should render with the native ACP tool header and invocation, got:\n{}",
                contents
            );
        }
        Err(e) => {
            panic!(
                "Tool call not rendered in TUI. Error: {}. Screen contents:\n{}",
                e,
                session.screen_contents()
            );
        }
    }
}

/// Test that an ACP tool call completion is rendered
///
/// This test verifies that when a tool call completes:
/// 1. The tool cell reaches the completed ACP phase
/// 2. The invocation is preserved
/// 3. Any output artifact is displayed
#[test]
#[cfg(target_os = "linux")]
fn test_acp_tool_call_completion_rendered_in_tui() {
    // Configure mock agent to send a tool call with completion
    // The mock agent sends a hardcoded tool call with title "Reading configuration file"
    // and final text "Tool call completed successfully."
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_agent_env("MOCK_AGENT_SEND_TOOL_CALL", "1");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn in ACP mode");

    // Wait for startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Send a prompt
    session.send_str("Echo hello").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for the mock response which means the tool call has completed
    // The mock agent sends "Tool call completed successfully." as final text
    session
        .wait_for_text("Tool call completed successfully", Duration::from_secs(10))
        .expect("Should receive completion response");

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    let contents = session.screen_contents();

    assert!(
        contents.contains("Tool [completed]: Reading configuration file (read)")
            && contents.contains("Output: Configuration loaded successfully"),
        "Completed tool call should show the native ACP completed tool cell, got:\n{}",
        contents
    );
    insta::assert_snapshot!("acp_tool_call_echo", normalize_for_input_snapshot(contents));
}

/// Test that ACP tool calls do NOT appear twice as separate active/completed cells
///
/// This test verifies that when a tool call completes, there is only ONE entry
/// in the TUI output, not duplicate entries showing both active and completed
/// ACP tool states for the same call.
///
/// ## Bug being tested:
/// When agent text streams while a tool call is active, the incomplete ACP tool
/// cell gets flushed to history. Then when the tool call completes, a new cell
/// is created, resulting in duplicate entries:
/// 1. An active ACP tool cell
/// 2. A duplicate completed ACP tool cell
///
/// This test uses MOCK_AGENT_INTERLEAVED_TOOL_CALL which sends text DURING
/// the tool call to trigger this exact scenario.
#[test]
#[cfg(target_os = "linux")]
fn test_acp_tool_call_no_duplicate_messages() {
    // Configure mock agent to send interleaved text and tool calls
    // This triggers the bug by sending text DURING the tool call execution
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_agent_env("MOCK_AGENT_INTERLEAVED_TOOL_CALL", "1");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn in ACP mode");

    // Wait for startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Send a prompt to trigger the interleaved tool call
    session.send_str("Test interleaved").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for the final text which means everything completed
    session
        .wait_for_text("Interleaved test done", Duration::from_secs(10))
        .expect("Should receive completion response");

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    let contents = session.screen_contents();

    // Count occurrences of the tool title "Executing interleaved command"
    // It should appear exactly ONCE in the completed ACP tool cell.
    let tool_title = "Executing interleaved command";
    let count = contents.matches(tool_title).count();

    assert_eq!(
        count, 1,
        "Tool call '{}' should appear exactly once, but appeared {} times.\n\
         This indicates duplicate messages (both active and completed states visible).\n\
         Screen contents:\n{}",
        tool_title, count, contents
    );

    assert!(
        contents.contains("Tool [completed]: Executing interleaved command (execute)"),
        "Should show the completed ACP tool state. Screen contents:\n{}",
        contents
    );

    // Verify we don't have both an active ACP tool state and a completed ACP
    // tool state for this tool call.
    let has_active = contents.lines().any(|line| {
        (line.contains("Tool [pending]")
            || line.contains("Tool [pending approval]")
            || line.contains("Tool [in progress]"))
            && line.contains("Executing interleaved")
    });
    let has_completed = contents
        .lines()
        .any(|line| line.contains("Tool [completed]") && line.contains("Executing interleaved"));

    assert!(
        !(has_active && has_completed),
        "Should NOT have both active and completed ACP tool states for the same tool call.\n\
         This indicates duplicate messages.\n\
         Screen contents:\n{}",
        contents
    );

    // Snapshot for visual verification
    insta::assert_snapshot!(
        "acp_tool_call_no_duplicates",
        normalize_for_input_snapshot(contents)
    );
}

/// Snapshot test for ACP tool call rendering
///
/// This captures the exact visual rendering of an ACP tool call
/// to detect any regressions in the display format.
#[test]
#[cfg(target_os = "linux")]
fn test_acp_tool_call_snapshot() {
    // Use the correct env var to trigger tool calls
    // The mock agent sends hardcoded content: title "Reading configuration file"
    // and final text "Tool call completed successfully."
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_agent_env("MOCK_AGENT_SEND_TOOL_CALL", "1");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn in ACP mode");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Send prompt to trigger tool call
    session.send_str("Read test.txt").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for the response - mock agent sends "Tool call completed successfully."
    session
        .wait_for_text("Tool call completed successfully", Duration::from_secs(10))
        .expect("Should receive response");

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);
    insta::assert_snapshot!(
        "acp_tool_call_read",
        normalize_for_input_snapshot(session.screen_contents())
    );
}

/// Test that multi-call exploring cells don't disappear when completed out-of-order.
///
/// This test verifies the fix for cells disappearing when:
/// 1. Multiple exploring tool calls (Read/Search) are emitted in one turn
/// 2. Agent text streams during execution, causing deferral pressure
/// 3. Completion events arrive out-of-order (e.g., call-2 completes before call-1)
///
/// The completed ACP tool cells should remain visible and complete correctly
/// even in this scenario.
#[test]
#[cfg(target_os = "linux")]
fn test_multi_call_exploring_cells_with_out_of_order_completion() {
    // Configure mock agent to send multiple exploring tool calls with interleaved text
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_agent_env("MOCK_AGENT_MULTI_CALL_EXPLORING", "1");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn codex in ACP mode");

    // Wait for startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Send a prompt to trigger the multi-call exploring sequence
    session.send_str("Explore files").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for task to start
    session
        .wait_for_text("Reading multiple files", Duration::from_secs(5))
        .expect("Should see the interleaved text message");

    // Wait for the final text which means everything completed
    session
        .wait_for_text("Multi-call exploring done", Duration::from_secs(10))
        .expect("Should receive completion response");

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    let contents = session.screen_contents();

    // Verify that all 3 Read operations appear in the output.
    assert!(
        contents.contains("file1.rs"),
        "Should show first Read operation. Screen contents:\n{}",
        contents
    );
    assert!(
        contents.contains("file2.rs"),
        "Should show second Read operation. Screen contents:\n{}",
        contents
    );
    assert!(
        contents.contains("file3.rs"),
        "Should show third Read operation. Screen contents:\n{}",
        contents
    );

    // Verify each completed exploring snapshot renders as a completed ACP tool
    // cell.
    assert!(
        contents.contains("Tool [completed]: Reading file1.rs (read)")
            && contents.contains("Tool [completed]: Reading file2.rs (read)")
            && contents.contains("Tool [completed]: Reading file3.rs (read)"),
        "Should show completed ACP tool cells for each file. Screen contents:\n{}",
        contents
    );

    // Count how many completed tool headers appear for the three read calls.
    let completed_count = contents.matches("Tool [completed]: Reading file").count();
    assert!(
        completed_count == 3,
        "Should have three completed ACP tool cells, found {}. Screen contents:\n{}",
        completed_count,
        contents
    );

    // CRITICAL: Verify the completed tool cells appear BEFORE the final agent
    // message rather than after it.
    let final_msg_pos = contents
        .find("Multi-call exploring done")
        .expect("Should contain final message");
    let file1_pos = contents
        .find("Reading file1.rs")
        .expect("Should contain file1");
    let file2_pos = contents
        .find("Reading file2.rs")
        .expect("Should contain file2");
    let file3_pos = contents
        .find("Reading file3.rs")
        .expect("Should contain file3");

    assert!(
        file1_pos < final_msg_pos && file2_pos < final_msg_pos && file3_pos < final_msg_pos,
        "Completed ACP tool cells should appear BEFORE the final agent message, not after. \
         file1 at {file1_pos}, file2 at {file2_pos}, file3 at {file3_pos}, final at {final_msg_pos}"
    );

    // Snapshot for visual verification
    insta::assert_snapshot!(
        "acp_multi_call_exploring",
        normalize_for_input_snapshot(contents)
    );
}

/// Test that exploring cells are flushed immediately even without subsequent agent text.
///
/// This is a regression test for a bug where completed ACP exploring tool cells
/// would remain in `active_cell` until task cleanup drained them instead of
/// becoming visible immediately.
#[test]
#[cfg(target_os = "linux")]
fn test_exploring_cell_flushed_immediately_without_agent_text() {
    // Configure mock agent with NO final text after tool calls complete
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_agent_env("MOCK_AGENT_MULTI_CALL_EXPLORING", "1")
        .with_agent_env("MOCK_AGENT_NO_FINAL_TEXT", "1");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn codex in ACP mode");

    // Wait for startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Send a prompt to trigger the multi-call exploring sequence
    session.send_str("Explore files").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for the completed ACP tool cells to appear even without subsequent
    // agent text.
    session
        .wait_for(
            |screen| {
                screen.contains("Tool [completed]: Reading file1.rs (read)")
                    && screen.contains("Tool [completed]: Reading file2.rs (read)")
                    && screen.contains("Tool [completed]: Reading file3.rs (read)")
            },
            Duration::from_secs(10),
        )
        .expect("Completed ACP tool cells should appear immediately after tool calls complete");

    let contents = session.screen_contents();

    // The critical assertion: the completed ACP tool cells MUST appear in the
    // output even though no agent text was sent after the tool calls completed.
    assert!(
        contents.contains("Tool [completed]: Reading file1.rs (read)")
            && contents.contains("Tool [completed]: Reading file2.rs (read)")
            && contents.contains("Tool [completed]: Reading file3.rs (read)"),
        "Completed ACP tool cells must appear immediately after tool calls complete, \
         even without subsequent agent text. If this fails, the cell is stuck \
         in active_cell until drain_failed(). Screen contents:\n{}",
        contents
    );
}

/// Test that tool call completions arriving DURING the final text stream are NOT
/// rendered after the agent's response.
///
/// ## The race condition:
/// When tool call completions arrive while the stream_controller is active (text is
/// streaming), they get deferred into the interrupt queue. Previously, on_task_complete()
/// would flush all deferred tool events, rendering them below the final
/// agent text. This creates a confusing UX where completed ACP tool cells appear
/// after the message the user needs to respond to.
///
/// ## Expected behavior (after fix):
/// Tool events still in the interrupt queue at task completion should be silently
/// discarded. The agent's final text should be the last thing visible.
///
/// ## Scenario:
/// 1. Agent sends 2 Read operations that complete before text (renders normally)
/// 2. Agent starts streaming final text (activates stream_controller)
/// 3. While text streams, 3 more Read/Search completions arrive (get deferred)
/// 4. Agent finishes text, turn ends
/// 5. Deferred tool events should NOT appear after the final text
#[test]
#[cfg(target_os = "linux")]
fn test_tool_calls_during_final_stream_not_shown_after() {
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_agent_env("MOCK_AGENT_TOOL_CALLS_DURING_FINAL_STREAM", "1");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn codex in ACP mode");

    // Wait for startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Send a prompt to trigger the race condition scenario
    session.send_str("Analyze the codebase").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for the final text to appear
    session
        .wait_for_text(
            "Let me know if you need anything else",
            Duration::from_secs(10),
        )
        .expect("Should receive final assistant message");

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    let contents = session.screen_contents();

    // The visible ACP tool cells should appear ABOVE the agent text since they
    // completed before streaming finished.
    assert!(
        contents.contains("Tool [completed]: Reading SKILL.md (read)")
            && contents.contains("Tool [completed]: Searching for undefined (search)")
            && contents.contains("Tool [completed]: Reading config.toml (read)"),
        "Should show the visible ACP tool cells before the final message. Screen contents:\n{}",
        contents
    );

    // Find the position of the final agent text
    let final_msg = "Let me know if you need anything else";
    let final_msg_pos = contents
        .find(final_msg)
        .expect("Should contain final message");

    // CRITICAL ASSERTION: No tool output should appear AFTER the final agent
    // message.
    let after_final = &contents[final_msg_pos + final_msg.len()..];
    let has_trailing_tool_output = after_final.contains("Tool [")
        || after_final.contains("SKILL.md")
        || after_final.contains("undefined")
        || after_final.contains("config.toml");

    assert!(
        !has_trailing_tool_output,
        "Tool output should NOT appear after the final agent message.\n\
         The deferred tool events from the interrupt queue should be discarded at task completion.\n\
         Text after final message:\n{after_final}\n\
         Full screen contents:\n{contents}",
    );

    // Snapshot for visual verification
    insta::assert_snapshot!(
        "acp_tool_calls_during_final_stream",
        normalize_for_input_snapshot(contents)
    );
}

/// Test that orphan tool cells are NOT created when deferred earlier snapshots
/// are discarded but terminal snapshots are still processed.
///
/// ## The bug (cascade deferral → orphan cells):
/// 1. Tool A snapshot → handled immediately (no stream active)
/// 2. Text streaming starts → stream_controller = Some
/// 3. Tool A terminal snapshot arrives → DEFERRED (stream active), queue becomes non-empty
/// 4. Tool B pending snapshot arrives → flush_answer_stream_with_separator() clears stream,
///    BUT !interrupts.is_empty() → DEFERRED (cascade deferral)
/// 5. Tool B terminal snapshot arrives → DEFERRED
/// 6. Turn ends → flush_completions_and_clear():
///    - Tool A terminal snapshot: processed OK
///    - Tool B pending snapshot: DISCARDED
///    - Tool B terminal snapshot: must also be discarded
///
/// ## Expected behavior (after fix):
/// Terminal snapshots whose earlier state was discarded should also be discarded.
/// No raw call_id should appear in the TUI output.
#[test]
#[cfg(target_os = "linux")]
fn test_no_orphan_tool_cells_from_cascade_deferral() {
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_agent_env("MOCK_AGENT_ORPHAN_TOOL_CELLS", "1");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn codex in ACP mode");

    // Wait for startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Send a prompt to trigger the orphan tool cell scenario
    session.send_str("Analyze code").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for the final text to appear
    session
        .wait_for_text("final analysis result", Duration::from_secs(10))
        .expect("Should receive final assistant message");

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    let contents = session.screen_contents();

    // CRITICAL ASSERTION: The raw call_id "orphan-tool-b" must NOT appear
    // in the rendered output. If it does, an orphan ACP tool cell was created
    // because flush_completions_and_clear processed a terminal snapshot after
    // discarding the earlier state for the same call_id.
    assert!(
        !contents.contains("orphan-tool-b"),
        "Raw call_id 'orphan-tool-b' should NOT appear in TUI output.\n\
         This indicates an orphan ACP tool cell was created from a discarded earlier snapshot.\n\
         Screen contents:\n{contents}",
    );

    // Tool A was handled correctly (Begin processed immediately, so End finds
    // it in running_commands). Its output should appear in completed form.
    // What we care about is that tool B's raw call_id doesn't appear.

    // The final text should be present
    assert!(
        contents.contains("final analysis result"),
        "Should show the final agent message. Screen contents:\n{contents}",
    );

    // Snapshot for visual verification
    insta::assert_snapshot!(
        "acp_no_orphan_tool_cells",
        normalize_for_input_snapshot(contents)
    );
}

/// Test that incomplete (stuck) tool calls don't block the agent's final message
/// from rendering.
///
/// ## The bug being tested:
/// When ACP terminal tool updates arrive on a separate async channel from the
/// agent's PromptResponse, the `turn_finished` gate in `on_agent_message()`
/// discards them for already-started tool calls. This leaves incomplete ACP
/// tool cells stuck in `active_cell`, filling the viewport and blocking
/// `insert_history_lines()` from rendering the agent's text response.
///
/// The user would see many tool calls "frozen" on screen with no agent response,
/// and only after manually interrupting would the previous message appear.
///
/// ## Scenario:
/// 1. Agent sends 3 Read tool calls (Begin only, no completion)
/// 2. Agent sends final text response
/// 3. Turn ends without tool completions
///
/// ## Expected behavior (after fix):
/// `finalize_active_cell_as_failed()` cleans up incomplete cells on agent message,
/// freeing the viewport so the agent's text renders.
#[test]
#[cfg(target_os = "linux")]
fn test_stuck_tool_calls_dont_block_agent_message() {
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_agent_env("MOCK_AGENT_STUCK_TOOL_CALLS", "1");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn in ACP mode");

    // Wait for startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Send a prompt to trigger the stuck tool call scenario
    session.send_str("Analyze files").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // CRITICAL ASSERTION: The agent's final text MUST appear.
    // If the bug is present, this will timeout because the stuck ACP tool cells
    // block the viewport and prevent the agent text from rendering.
    session
        .wait_for_text(
            "Analysis complete despite incomplete tool calls",
            Duration::from_secs(10),
        )
        .expect(
            "Agent message MUST render even when tool calls don't complete. \
             If this times out, ACP tool cells are blocking the viewport.",
        );

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    let contents = session.screen_contents();

    // The agent's response must be visible
    assert!(
        contents.contains("Analysis complete despite incomplete tool calls"),
        "Agent message should be visible in the TUI output. Screen contents:\n{contents}",
    );

    // The prompt indicator should return (turn is over)
    assert!(
        contents.contains("›"),
        "Prompt indicator should be visible after turn completes. Screen contents:\n{contents}",
    );

    // Snapshot for visual verification
    insta::assert_snapshot!(
        "acp_stuck_tool_calls_agent_message_renders",
        normalize_for_input_snapshot(contents)
    );
}

/// Test that a generic tool call (no raw_input) displays a resolved semantic name
/// instead of the raw tool call ID.
///
/// ## The bug:
/// When an ACP ToolCall has a generic title ("Terminal") and no raw_input, the
/// completion path must still preserve that resolved title instead of falling
/// back to the raw `toolu_` ID.
///
/// ## Expected behavior:
/// The TUI should show "Terminal" or a similar resolved name instead of
/// "toolu_generic_test_001".
#[test]
#[cfg(target_os = "linux")]
fn test_acp_generic_tool_call_shows_resolved_name() {
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_agent_env("MOCK_AGENT_GENERIC_TOOL_CALL", "1");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn in ACP mode");

    // Wait for startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Send a prompt to trigger the generic tool call
    session.send_str("Run a command").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for the final text which means the tool call has completed
    session
        .wait_for_text("Generic tool call done", Duration::from_secs(10))
        .expect("Should receive completion response");

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    let contents = session.screen_contents();

    // CRITICAL ASSERTION: The raw tool call ID must NOT appear in the output.
    assert!(
        !contents.contains("toolu_generic_test_001"),
        "Raw tool call ID 'toolu_generic_test_001' should NOT appear in TUI output.\n\
         The TUI should display the resolved tool name instead.\n\
         Screen contents:\n{contents}",
    );

    // The resolved name should be visible in the rendered output
    assert!(
        contents.contains("Tool [completed]: Terminal (execute)"),
        "Should display the resolved tool name 'Terminal' in the native ACP tool cell.\n\
         Screen contents:\n{contents}",
    );

    // Snapshot captures the exact rendering for regression detection
    insta::assert_snapshot!(
        "acp_generic_tool_call_resolved_name",
        normalize_for_input_snapshot(contents)
    );
}

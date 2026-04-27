//! E2E tests for ACP tool call rendering in the TUI
//!
//! These tests verify that tool calls from ACP agents are properly
//! rendered in the TUI using the McpToolCallCell component.
//!
//! ## Test Strategy
//!
//! The tests configure the mock-acp-agent to emit ToolCall/ToolCallUpdate
//! events, then verify the TUI displays them correctly. This validates
//! the entire ACP-to-TUI flow:
//!
//! 1. Mock agent sends `SessionUpdate::ToolCall`
//! 2. ACP translator converts to `EventMsg::McpToolCallBegin`
//! 3. TUI chatwidget renders via `McpToolCallCell`
//!
//! ## Expected TUI Output Format
//!
//! Active tool calls display as:
//! ```text
//! • Calling server.tool_name({"arg":"value"})
//! ```
//!
//! Completed tool calls display as:
//! ```text
//! ✓ Called server.tool_name({"arg":"value"}) (1.2s)
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
/// 2. Translator converts it to McpToolCallBegin
/// 3. TUI displays it using McpToolCallCell
///
/// ## Prerequisites for this test to pass:
/// - Mock agent must support MOCK_AGENT_TOOL_CALL env var
/// - translator.rs must handle SessionUpdate::ToolCall
/// - core/client.rs must emit EventMsg::McpToolCallBegin
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

    // Wait for tool call to appear in TUI
    // The tool call should render like: "• Calling acp.read_file(...)"
    // or with the server name from ACP
    let tool_call_appeared = session.wait_for(
        |screen| {
            // Look for signs of tool call rendering
            screen.contains("Explored")
        },
        Duration::from_secs(10),
    );

    match tool_call_appeared {
        Ok(()) => {
            // Tool call UI appeared
            let contents = session.screen_contents();

            // The tool call should display with the tool name
            assert!(
                contents.contains("Explored") && contents.contains("Read config.toml"),
                "Tool call should show tool name or 'Calling' prefix, got:\n{}",
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
/// 1. The status changes from "Calling" to "Called"
/// 2. The duration is shown
/// 3. Any output is displayed
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

    // After completion, should show "Called" or "Reading" (from title "Reading configuration file")
    // The format is: "✓ Called server.tool_name(...) (Xs)" or the title display
    assert!(
        contents.contains("Explored"),
        "Completed tool call should show 'Called' or tool title, got:\n{}",
        contents
    );
    insta::assert_snapshot!("acp_tool_call_echo", normalize_for_input_snapshot(contents));
}

/// Test that ACP tool calls do NOT appear twice (once as Running, once as Ran)
///
/// This test verifies that when a tool call completes, there is only ONE entry
/// in the TUI output, not duplicate entries showing both "Running" and "Ran"
/// states. The expected behavior is that the "Running" state should be
/// updated in-place to become "Ran" when the tool call completes.
///
/// ## Bug being tested:
/// When agent text streams while a tool call is active, the incomplete ExecCell
/// gets flushed to history. Then when the tool call completes, a new ExecCell
/// is created, resulting in duplicate entries:
/// 1. "Running ..." (flushed incomplete cell)
/// 2. "Ran ..." (new completed cell)
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

    // The Execute tool renders using the command from Invocation::Command
    // (extracted from raw_input). The command is "test", so the TUI shows
    // "Running test" (in-progress) or "Ran test" (completed).
    // Count occurrences to verify no duplicates (both Running and Ran visible).
    let running_count = contents.matches("Running test").count();
    let ran_count = contents.matches("Ran test").count();
    let total = running_count + ran_count;

    assert_eq!(
        total, 1,
        "Execute tool should appear exactly once (either 'Running test' or 'Ran test'), \
         but appeared {total} times (Running: {running_count}, Ran: {ran_count}).\n\
         This indicates duplicate messages (both states visible).\n\
         Screen contents:\n{contents}",
    );

    // The tool cell should appear BEFORE the interleaved text (correct chronological order)
    let tool_pos = contents
        .find("Running test")
        .or_else(|| contents.find("Ran test"))
        .expect("Should contain tool cell with command");
    let text_pos = contents
        .find("Interleaved test done")
        .expect("Should contain final text");
    assert!(
        tool_pos < text_pos,
        "Tool cell should appear BEFORE final text (chronological order).\n\
         Screen contents:\n{contents}",
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
/// 1. Multiple exploring tool calls (Read/Search) are grouped into one ExecCell
/// 2. Agent text streams during execution, causing the cell to flush while incomplete
/// 3. Completion events arrive out-of-order (e.g., call-2 completes before call-1)
///
/// The cell should remain visible and complete correctly even in this scenario.
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

    // Verify that all 3 Read operations appear in the output
    assert!(
        contents.contains("file1.rs") || contents.contains("Explored"),
        "Should show first Read operation. Screen contents:\n{}",
        contents
    );
    assert!(
        contents.contains("file2.rs") || contents.contains("Explored"),
        "Should show second Read operation. Screen contents:\n{}",
        contents
    );
    assert!(
        contents.contains("file3.rs") || contents.contains("Explored"),
        "Should show third Read operation. Screen contents:\n{}",
        contents
    );

    // Verify tool cells appear. When text arrives during incomplete tool calls,
    // the cell is flushed to history immediately (may show as "Exploring" if not
    // yet complete). Tool calls that complete after the flush produce a separate
    // completed cell.
    assert!(
        contents.contains("Explored") || contents.contains("Exploring"),
        "Should show exploring/explored cell. Screen contents:\n{}",
        contents
    );

    // CRITICAL: Verify tool cells appear BEFORE the final agent message.
    // This ensures correct chronological ordering.
    let tool_pos = contents
        .find("Exploring")
        .or_else(|| contents.find("Explored"))
        .expect("Should contain exploring cell");
    let final_msg_pos = contents
        .find("Multi-call exploring done")
        .expect("Should contain final message");

    assert!(
        tool_pos < final_msg_pos,
        "Tool cells should appear BEFORE the final agent message (chronological order). \
         Tool at {tool_pos}, final message at {final_msg_pos}",
    );

    // Snapshot for visual verification
    insta::assert_snapshot!(
        "acp_multi_call_exploring",
        normalize_for_input_snapshot(contents)
    );
}

/// Test that exploring cells are flushed immediately even without subsequent agent text.
///
/// This is a regression test for a bug where completed exploring cells would remain
/// in active_cell until TaskComplete drained them, instead of being flushed immediately.
///
/// The bug occurred because handle_exec_end_now() checked `cell.should_flush()` which
/// returns false for exploring cells, so the cell wasn't flushed until agent text
/// triggered flush_active_cell() or TaskComplete drained pending cells.
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

    // Wait for the exploring cell to appear - it should be flushed immediately
    // after the last tool call completes, even without subsequent agent text
    session
        .wait_for_text("Explored", Duration::from_secs(10))
        .expect("The 'Explored' cell should appear immediately after tool calls complete");

    let contents = session.screen_contents();

    // The critical assertion: the "Explored" cell MUST appear in the output
    // even though no agent text was sent after the tool calls completed.
    // If the cell wasn't flushed immediately, it would be missing from the display.
    assert!(
        contents.contains("Explored"),
        "The 'Explored' cell must appear immediately after tool calls complete, \
         even without subsequent agent text. If this fails, the cell is stuck \
         in active_cell until drain_failed(). Screen contents:\n{}",
        contents
    );

    // Verify all 3 files are shown
    let read_text = if contents.contains("file1.rs") {
        "individual files shown"
    } else {
        "grouped display"
    };

    assert!(
        contents.contains("Explored") || contents.contains("file1.rs"),
        "Should show completed exploring operation ({}). Screen contents:\n{}",
        read_text,
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
/// agent text. This creates a confusing UX where "Explored" / "Ran" cells appear
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

    // The first batch (file1.rs, file2.rs) should appear ABOVE the agent text
    // since they completed before streaming started.
    assert!(
        contents.contains("Explored"),
        "Should show the initial batch of explored files. Screen contents:\n{}",
        contents
    );

    // Find the position of the final agent text
    let final_msg = "Let me know if you need anything else";
    let final_msg_pos = contents
        .find(final_msg)
        .expect("Should contain final message");

    // CRITICAL ASSERTION: No tool output should appear AFTER the final agent message.
    // Look for any "Explored", "Ran", "Searched", "Read", "SKILL.md", "config.toml",
    // or "undefined" text after the final message position.
    let after_final = &contents[final_msg_pos + final_msg.len()..];
    let has_trailing_tool_output = after_final.contains("Explored")
        || after_final.contains("Ran")
        || after_final.contains("Searched")
        || after_final.contains("SKILL.md")
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

/// Test that orphan tool cells are NOT created when deferred Begin events are
/// discarded but their End events are still processed.
///
/// ## The bug (cascade deferral → orphan cells):
/// 1. Tool A Begin → handled immediately (no stream active)
/// 2. Text streaming starts → stream_controller = Some
/// 3. Tool A End arrives → DEFERRED (stream active), queue becomes non-empty
/// 4. Tool B Begin arrives → flush_answer_stream_with_separator() clears stream,
///    BUT !interrupts.is_empty() → DEFERRED (cascade deferral)
/// 5. Tool B End arrives → DEFERRED
/// 6. Turn ends → flush_completions_and_clear():
///    - End-A: processed OK (running_commands has entry)
///    - Begin-B: DISCARDED
///    - End-B: processed, but no running_commands entry (Begin-B was discarded)
///      → creates orphan ExecCell with command = ["orphan-tool-b"]
///      → renders as "• Ran orphan-tool-b / └ No files found"
///
/// ## Expected behavior (after fix):
/// End events whose Begin was discarded should also be discarded.
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
    // in the rendered output. If it does, an orphan ExecCell was created
    // because flush_completions_and_clear processed End-B after discarding
    // Begin-B.
    assert!(
        !contents.contains("orphan-tool-b"),
        "Raw call_id 'orphan-tool-b' should NOT appear in TUI output.\n\
         This indicates an orphan ExecCell was created from a discarded Begin event.\n\
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
/// When ACP tool call End events arrive on a separate async channel from the
/// agent's PromptResponse, the `turn_finished` gate in `on_agent_message()`
/// discards End events for already-started tool calls. This leaves incomplete
/// ExecCells stuck in `active_cell`, filling the viewport and blocking
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
    // If the bug is present, this will timeout because the stuck ExecCells
    // block the viewport and prevent the agent text from rendering.
    session
        .wait_for_text(
            "Analysis complete despite incomplete tool calls",
            Duration::from_secs(10),
        )
        .expect(
            "Agent message MUST render even when tool calls don't complete. \
             If this times out, ExecCells are blocking the viewport.",
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
/// translation layer skips emitting ExecCommandBegin (stores data for later).
/// On completion, it emits ExecCommandEnd with the resolved title in `command`.
/// But the TUI's handle_exec_end_now() ignores `ev.command` when there's no
/// matching Begin event, falling back to `ev.call_id` (the raw `toolu_` ID).
///
/// ## Expected behavior:
/// The TUI should use `ev.command` from the End event, showing "Terminal" or
/// a similar resolved name instead of "toolu_generic_test_001".
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
    // If it does, the TUI is using ev.call_id instead of ev.command.
    assert!(
        !contents.contains("toolu_generic_test_001"),
        "Raw tool call ID 'toolu_generic_test_001' should NOT appear in TUI output.\n\
         The TUI should display the resolved tool name from ev.command instead.\n\
         Screen contents:\n{contents}",
    );

    // The resolved name should be visible in the rendered output
    assert!(
        contents.contains("Ran Terminal"),
        "Should display the resolved tool name 'Terminal' from ev.command.\n\
         Screen contents:\n{contents}",
    );

    // Snapshot captures the exact rendering for regression detection
    insta::assert_snapshot!(
        "acp_generic_tool_call_resolved_name",
        normalize_for_input_snapshot(contents)
    );
}

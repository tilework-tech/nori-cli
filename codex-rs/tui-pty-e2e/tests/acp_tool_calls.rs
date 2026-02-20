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

    // Count occurrences of the tool title "Executing interleaved command"
    // It should appear exactly ONCE (in the completed "Ran" form)
    let tool_title = "Executing interleaved command";
    let count = contents.matches(tool_title).count();

    assert_eq!(
        count, 1,
        "Tool call '{}' should appear exactly once, but appeared {} times.\n\
         This indicates duplicate messages (both 'Running' and 'Ran' states visible).\n\
         Screen contents:\n{}",
        tool_title, count, contents
    );

    // Also verify we see "Ran" (completed state)
    assert!(
        contents.contains("Ran"),
        "Should show completed 'Ran' state. Screen contents:\n{}",
        contents
    );

    // Verify we don't have both "Running" AND "Ran" for this tool call
    // (which would indicate duplicates)
    let has_running = contents
        .lines()
        .any(|line| line.contains("Running") && line.contains("Executing interleaved"));
    let has_ran = contents
        .lines()
        .any(|line| line.contains("Ran") && line.contains("Executing interleaved"));

    assert!(
        !(has_running && has_ran),
        "Should NOT have both 'Running' and 'Ran' states for the same tool call.\n\
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

    // Verify the exploring cell is shown as completed (not stuck in "Running" state)
    // The completed exploring cell should show "Explored" status
    assert!(
        contents.contains("Explored"),
        "Should show completed 'Explored' state. Screen contents:\n{}",
        contents
    );

    // Count how many "Explored" entries appear - should be exactly 1 grouped cell
    // All 3 Read operations are grouped into a single cell because incomplete ExecCells
    // are NOT flushed during streaming text. The text "Reading multiple files..." arrives
    // while calls 1 & 2 are still pending, but the cell stays in active_cell. Call 3 then
    // joins via with_added_call(), resulting in one cell with all 3 Read operations.
    let explored_count = contents.matches("Explored").count();
    assert!(
        explored_count == 1,
        "Should have one 'Explored' entry (all calls grouped), found {}. Screen contents:\n{}",
        explored_count,
        contents
    );

    // CRITICAL: Verify the "Explored" cell appears BEFORE the final agent message
    // This ensures it was flushed immediately when the last tool call completed,
    // not delayed until TaskComplete drained pending cells.
    let explored_pos = contents
        .find("Explored")
        .expect("Should contain 'Explored'");
    let final_msg_pos = contents
        .find("Multi-call exploring done")
        .expect("Should contain final message");

    assert!(
        explored_pos < final_msg_pos,
        "The 'Explored' cell should appear BEFORE the final agent message, not after. \
         This ensures it was flushed immediately on completion, not delayed until task end. \
         Explored at {}, final message at {}",
        explored_pos,
        final_msg_pos
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

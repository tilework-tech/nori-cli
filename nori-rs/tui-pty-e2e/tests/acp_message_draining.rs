//! E2E tests to reproduce the ACP message draining bug
//!
//! Bug description: Agent messages don't fully output in ACP mode - they only
//! "drain" when the user types their next prompt. The text appears instantly
//! (not streaming), as if it was buffered somewhere. The conversation feels
//! "off by one" - desynchronized between what the agent sees vs what the user sees.
//!
//! These tests attempt to reproduce the bug by sending multiple prompts and
//! verifying that responses appear at the correct time.

use std::time::Duration;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TuiSession;

/// Test that a single prompt receives its response before the user sends the next prompt
///
/// This is the most basic test - send one prompt, verify response appears.
#[test]
#[cfg(target_os = "linux")]
fn test_single_prompt_response_appears_immediately() {
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_mock_response("RESPONSE_ONE_UNIQUE_MARKER");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn in ACP mode");

    // Wait for startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Send first prompt
    session.send_str("First prompt").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Response should appear WITHOUT needing to send another prompt
    session
        .wait_for_text("RESPONSE_ONE_UNIQUE_MARKER", Duration::from_secs(10))
        .expect("Response should appear immediately after first prompt, not require second prompt");

    eprintln!(
        "Single prompt test passed. Screen:\n{}",
        session.screen_contents()
    );
}

/// Test that sending two prompts shows each response with its corresponding prompt
///
/// This test checks for the "off by one" bug:
/// - Send prompt 1, expect response 1 to appear
/// - Send prompt 2, expect response 2 to appear (NOT response 1!)
#[test]
#[cfg(target_os = "linux")]
fn test_two_prompts_responses_not_off_by_one() {
    // We need to use agent env vars to control responses per-prompt
    // Since mock agent doesn't support that directly, we'll check ordering
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_mock_response("FIRST_RESPONSE_MARKER");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn in ACP mode");

    // Wait for startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Send first prompt
    session.send_str("First prompt").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // First response should appear before we send second prompt
    let first_response_appeared =
        session.wait_for_text("FIRST_RESPONSE_MARKER", Duration::from_secs(10));

    match first_response_appeared {
        Ok(()) => {
            eprintln!(
                "First response appeared correctly. Screen:\n{}",
                session.screen_contents()
            );
        }
        Err(e) => {
            // This is the bug! Response didn't appear until we would send next prompt
            panic!(
                "BUG REPRODUCED: First response did not appear after first prompt. \
                 This is the 'off by one' draining bug. Error: {}. Screen:\n{}",
                e,
                session.screen_contents()
            );
        }
    }

    // Wait for input to be ready again
    session
        .wait_for_text("›", Duration::from_secs(5))
        .expect("Should return to input state");

    std::thread::sleep(TIMEOUT_INPUT);

    // Send second prompt (note: mock agent will send same response)
    session.send_str("Second prompt").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Second response should appear
    // Since it's the same text, we just verify the prompt cycle completes
    session
        .wait_for_text("›", Duration::from_secs(10))
        .expect("Should complete second prompt cycle");

    eprintln!(
        "Two prompts test completed. Screen:\n{}",
        session.screen_contents()
    );
}

/// Test with a longer response that might stress the channel buffering
///
/// The bug description mentions that text appears "instantly" when it finally
/// shows, suggesting buffering. A longer response might make this more visible.
#[test]
#[cfg(target_os = "linux")]
fn test_long_response_appears_immediately() {
    // Create a longer response that might stress buffering
    let long_response = "LINE_1_OF_LONG_RESPONSE\n\
                         LINE_2_OF_LONG_RESPONSE\n\
                         LINE_3_OF_LONG_RESPONSE\n\
                         LINE_4_OF_LONG_RESPONSE\n\
                         LINE_5_OF_LONG_RESPONSE\n\
                         FINAL_LINE_MARKER";

    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_mock_response(long_response);

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn in ACP mode");

    // Wait for startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Send prompt
    session.send_str("Give me a long response").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // The FINAL line should appear without needing another prompt
    session
        .wait_for_text("FINAL_LINE_MARKER", Duration::from_secs(10))
        .expect("Full response including final line should appear immediately");

    let contents = session.screen_contents();

    // Verify all lines are present
    assert!(
        contents.contains("LINE_1_OF_LONG_RESPONSE"),
        "Should contain first line"
    );
    assert!(
        contents.contains("FINAL_LINE_MARKER"),
        "Should contain final line"
    );

    eprintln!("Long response test passed. Screen:\n{}", contents);
}

/// Test with delays between chunks to simulate realistic streaming
///
/// This tests whether the draining issue is related to timing/streaming.
#[test]
#[cfg(target_os = "linux")]
fn test_delayed_streaming_response() {
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_mock_response("DELAYED_RESPONSE_MARKER")
        // Add delay between chunks to simulate realistic streaming
        .with_agent_env("MOCK_AGENT_DELAY_MS", "100");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn in ACP mode");

    // Wait for startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Send prompt
    session.send_str("Delayed response test").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Response should appear even with the delay
    session
        .wait_for_text("DELAYED_RESPONSE_MARKER", Duration::from_secs(15))
        .expect("Delayed response should still appear without needing another prompt");

    eprintln!(
        "Delayed streaming test passed. Screen:\n{}",
        session.screen_contents()
    );
}

/// Test with tool calls to see if they affect message draining
///
/// Tool calls involve more complex event flow, which might expose the bug.
#[test]
#[cfg(target_os = "linux")]
fn test_tool_call_response_draining() {
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_tool_call(); // This sets MOCK_AGENT_SEND_TOOL_CALL=1

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn in ACP mode");

    // Wait for startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Send prompt
    session.send_str("Do a tool call").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Should see both the tool call AND the final message
    // The mock agent sends "Tool call completed successfully." after the tool call
    session
        .wait_for_text("Tool call completed successfully", Duration::from_secs(15))
        .expect("Final message after tool call should appear without needing another prompt");

    eprintln!(
        "Tool call draining test passed. Screen:\n{}",
        session.screen_contents()
    );
}

/// Interactive debugging test - keeps session open for manual inspection
///
/// Run with: cargo test -p tui-pty-e2e test_interactive_debug -- --ignored --nocapture
/// Then examine the output to see what's happening.
#[test]
#[ignore]
fn test_interactive_debug_draining() {
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_mock_response("DEBUG_RESPONSE_1");

    let mut session =
        TuiSession::spawn_with_config(40, 120, config).expect("Failed to spawn in ACP mode");

    // Wait for startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    eprintln!("=== Initial state ===");
    eprintln!("{}", session.screen_contents());
    eprintln!("=====================");

    std::thread::sleep(TIMEOUT_INPUT);

    // Send first prompt
    eprintln!("\n>>> Sending first prompt...");
    session.send_str("First prompt").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait a bit and show state
    std::thread::sleep(Duration::from_secs(2));
    eprintln!("\n=== After first prompt (2s wait) ===");
    eprintln!("{}", session.screen_contents());
    eprintln!("=====================================");

    // Check if response appeared
    let has_response = session.screen_contents().contains("DEBUG_RESPONSE_1");
    eprintln!("\nResponse appeared: {}", has_response);

    if !has_response {
        eprintln!(
            "\n>>> Response NOT visible. Sending second prompt to see if it triggers draining..."
        );
        std::thread::sleep(TIMEOUT_INPUT);
        session.send_str("Second prompt").unwrap();
        std::thread::sleep(TIMEOUT_INPUT);

        eprintln!("\n=== After typing second prompt (before Enter) ===");
        eprintln!("{}", session.screen_contents());
        eprintln!("=================================================");

        let has_response_now = session.screen_contents().contains("DEBUG_RESPONSE_1");
        eprintln!("\nResponse appeared after typing: {}", has_response_now);

        session.send_key(Key::Enter).unwrap();
        std::thread::sleep(Duration::from_secs(2));

        eprintln!("\n=== After sending second prompt ===");
        eprintln!("{}", session.screen_contents());
        eprintln!("====================================");
    }

    // Keep session alive for manual inspection
    eprintln!("\nTest complete. Keeping session alive for 5 more seconds...");
    std::thread::sleep(Duration::from_secs(5));
}

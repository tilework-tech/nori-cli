use insta::assert_snapshot;
use std::time::Duration;
use tui_pty_e2e::normalize_for_snapshot;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TuiSession;
use tui_pty_e2e::TIMEOUT;

#[test]
fn test_submit_text() {
    let config = SessionConfig::new().with_stream_until_cancel();

    let mut session = TuiSession::spawn_with_config(24, 80, config).unwrap();
    session.wait_for_text("To get started", TIMEOUT).unwrap();

    // Submit prompt
    session.send_str("testing!!!").unwrap();
    session.wait_for_text("testing!!!", TIMEOUT).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    session.send_key(Key::Enter).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    session.wait_for_text("GOOGLE_API_KEY", TIMEOUT).unwrap();

    assert_snapshot!(
        "submit_input",
        normalize_for_snapshot(session.screen_contents())
    );
}

// #[test]
// fn test_escape_cancels_streaming() {
//     let config = SessionConfig::new().with_stream_until_cancel();
//
//     let mut session = TuiSession::spawn_with_config(24, 80, config).unwrap();
//     session.wait_for_text("To get started", TIMEOUT).unwrap();
//
//     // Submit prompt
//     session.send_str("testing!!!").unwrap();
//     session.wait_for_text("testing!!!", TIMEOUT).unwrap();
//     std::thread::sleep(Duration::from_millis(100));
//     session.send_key(Key::Enter).unwrap();
//     std::thread::sleep(Duration::from_millis(100));
//
//     // Wait for streaming to start
//     session
//         .wait_for_text("Streaming...", TIMEOUT)
//         .expect("Streaming did not start");
//
//     // Press Escape to cancel
//     session.send_key(Key::Escape).unwrap();
//
//     // Verify cancellation completed
//     // (exact behavior depends on TUI implementation)
//     session
//         .wait_for(
//             |s| s.contains("Cancelled") || s.contains("Stopped"),
//             TIMEOUT,
//         )
//         .ok(); // May not show explicit message
//
//     assert_snapshot!(
//         "cancelled_stream",
//         normalize_for_snapshot(session.screen_contents())
//     )
// }

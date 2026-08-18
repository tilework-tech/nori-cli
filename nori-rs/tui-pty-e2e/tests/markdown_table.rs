//! End-to-end coverage for markdown tables arriving as a stream of small chunks.
//!
//! Table layout depends on every row: each new row can widen a column, which rewrites the header,
//! the header rule, and every separator already drawn. A transcript that commits rendered lines as
//! they stream must therefore withhold a table until it closes, or the finished transcript keeps
//! a raw `| header |` line and separators of several different widths.

use insta::assert_snapshot;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TIMEOUT_PRESNAPSHOT;
use tui_pty_e2e::TuiSession;
use tui_pty_e2e::normalize_for_input_snapshot;

const TABLE_RESPONSE: &str = concat!(
    "Here is a table:\n",
    "\n",
    "| Situation | Commit strategy |\n",
    "| --- | --- |\n",
    "| Streaming/tail append | Existing incremental insertion |\n",
    "| Width changes | Debounced full replay |\n",
    "| Rollback/session replacement/resume | Full replay |\n",
    "\n",
    "Done.\n",
);

#[test]
#[cfg(target_os = "linux")]
fn test_streamed_markdown_table_renders_as_a_grid() {
    // Eight characters per chunk splits every row across several updates, so the transcript sees
    // the table half-written many times over.
    let config = SessionConfig::new().with_mock_response_streamed(TABLE_RESPONSE, 8);
    let mut session = TuiSession::spawn_with_config(24, 100, config).expect("Failed to spawn");

    session.wait_for_text("›", TIMEOUT).unwrap();

    session.send_str("show me a table").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    session
        .wait_for_text("Done.", TIMEOUT)
        .expect("Did not receive the streamed table");
    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    let screen = session.screen_contents();
    assert!(
        !screen.contains("| Situation |"),
        "raw markdown table header survived streaming:\n{screen}"
    );
    assert!(
        screen.contains('━'),
        "expected a grid header rule:\n{screen}"
    );

    assert_snapshot!(
        "streamed_markdown_table",
        normalize_for_input_snapshot(screen)
    );
}

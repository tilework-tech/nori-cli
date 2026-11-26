#![cfg(unix)]

use std::time::Duration;
use std::time::Instant;

use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TuiSession;

#[test]
fn handles_input_while_palette_replies_are_pending() {
    let config = SessionConfig {
        no_color: false,
        ..SessionConfig::new()
    }
    .with_agent_env("COLORTERM", "truecolor")
    .with_agent_env(
        "MOCK_AGENT_EXPECT_LAST_PROMPT_TEXT_BLOCK",
        "λ café]10;rgb:aa/bb/cc!",
    )
    .with_mock_response("Palette draft delivered.");
    let mut session = TuiSession::spawn_with_config(24, 90, config).unwrap();
    session.withhold_palette_replies();

    let deadline = Instant::now() + TIMEOUT;
    while !session
        .raw_output()
        .windows(8)
        .any(|seq| seq == b"\x1b]10;?\x1b\\")
    {
        assert!(Instant::now() < deadline, "TUI did not request the palette");
        session.poll().unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }
    session.send_key(Key::Char('λ')).unwrap();
    session
        .wait_for_text("〉λ", Duration::from_millis(1500))
        .unwrap();
    assert!(
        session
            .raw_output()
            .windows(8)
            .any(|seq| seq == b"\x1b]11;?\x1b\\")
    );

    // The terminal answers in reverse order. The OSC-looking text between paste
    // delimiters belongs to the user and goes through the composer's paste sanitizer.
    session
        .send_str(concat!(
            "\x1b]11;rgb:00/11/22\x07",
            "\x1b[200~ café\x1b]10;rgb:aa/bb/cc\x07\x1b[201~",
            "\x1b]10;rgb:33/44/55\x1b\\",
            "!",
        ))
        .unwrap();
    session
        .wait_for_text("〉λ café]10;rgb:aa/bb/cc!", TIMEOUT)
        .unwrap();
    session
        .wait_for_text("Mock Default Model", TIMEOUT)
        .unwrap();
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("Palette draft delivered.", TIMEOUT)
        .unwrap();
}

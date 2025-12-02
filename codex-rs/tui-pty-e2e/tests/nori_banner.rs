//! E2E tests for the Nori welcome banner display.
//!
//! These tests verify that the Nori ASCII art banner is displayed
//! correctly during startup.

use insta::assert_snapshot;
use tui_pty_e2e::normalize_for_snapshot;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TuiSession;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;

#[test]
fn test_startup_shows_nori_banner() {
    let mut session = TuiSession::spawn_with_config(
        24,
        80,
        SessionConfig::default()
            // Don't include the values that would bypass welcome
            .without_approval_policy()
            .without_sandbox()
            .with_config_toml(""),
    )
    .expect("Failed to spawn codex");

    // Wait for the Nori ASCII art to appear
    session
        .wait_for_text("|_| \\_|", TIMEOUT)
        .expect("Nori ASCII art banner did not appear");
    std::thread::sleep(TIMEOUT_INPUT);

    let contents = session.screen_contents();

    // Verify Nori ASCII art is present (distinctive part of the logo)
    assert!(
        contents.contains("|_| \\_|"),
        "Expected Nori ASCII art banner, but got: {}",
        contents
    );

    // Verify the tagline is present
    assert!(
        contents.contains("powered by Nori"),
        "Expected Nori tagline, but got: {}",
        contents
    );

    assert_snapshot!(
        "startup_nori_banner",
        normalize_for_snapshot(session.screen_contents())
    );
}

#[test]
fn test_nori_banner_shows_profile() {
    let mut session = TuiSession::spawn_with_config(
        24,
        80,
        SessionConfig::default()
            .without_approval_policy()
            .without_sandbox()
            .with_config_toml(""),
    )
    .expect("Failed to spawn codex");

    // Wait for the Nori banner to appear
    session
        .wait_for_text("|_| \\_|", TIMEOUT)
        .expect("Nori ASCII art banner did not appear");
    std::thread::sleep(TIMEOUT_INPUT);

    let contents = session.screen_contents();

    // Verify profile line is displayed
    assert!(
        contents.contains("profile:"),
        "Expected profile line in banner, but got: {}",
        contents
    );

    assert_snapshot!(
        "nori_banner_profile",
        normalize_for_snapshot(session.screen_contents())
    );
}

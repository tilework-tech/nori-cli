use insta::assert_snapshot;
use std::time::Duration;
use std::time::Instant;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TIMEOUT_PRESNAPSHOT;
use tui_pty_e2e::TuiSession;
use tui_pty_e2e::normalize_for_input_snapshot;

#[test]
// Testing that ACP mode with a nonexistent model produces a clear error
// instead of falling back to HTTP providers
fn test_startup_error_for_unregistered_model() {
    let mut session = TuiSession::spawn_with_config(
        18,
        80,
        SessionConfig::new().with_model("nonexistent".to_owned()),
    )
    .expect("Failed to spawn codex");

    // When acp.allow_http_fallback=false (default) and the model is not registered as an ACP agent,
    // the TUI should show an error immediately at startup (not after prompt submission).
    // The error is shown before the TUI even renders the shortcuts prompt.
    session
        .wait_for_text(
            "Model 'nonexistent' is not registered as an ACP agent",
            TIMEOUT,
        )
        .unwrap();

    std::thread::sleep(TIMEOUT_INPUT);
    assert_snapshot!(
        "startup_error_unregistered_model",
        normalize_for_input_snapshot(session.screen_contents())
    );
}

#[test]
#[cfg(target_os = "linux")]
fn test_startup_shows_banner() {
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

    session
        .wait_for_text("Welcome to Codex", TIMEOUT)
        .expect("Prompt did not appear");

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    let contents = session.screen_contents();
    assert!(contents.contains("Welcome to Codex"));
    assert_snapshot!(
        "startup_shows_welcome",
        normalize_for_input_snapshot(contents)
    );
}

#[test]
#[cfg(target_os = "linux")]
fn test_startup_welcome_with_dimensions() {
    let mut session = TuiSession::spawn_with_config(
        40,
        120,
        SessionConfig::default()
            // Don't include the values that would bypass welcome
            .without_approval_policy()
            .without_sandbox(),
    )
    .expect("Failed to spawn codex");

    session
        .wait_for_text("Powered by Nori AI", TIMEOUT)
        .expect("Prompt did not appear");

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);
    std::thread::sleep(TIMEOUT_PRESNAPSHOT);
    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    // Verify terminal size is respected
    let contents = session.screen_contents();
    assert!(contents.lines().count() <= 40);

    assert_snapshot!(
        "startup_welcome_dimensions_40x120",
        normalize_for_input_snapshot(contents)
    );
}

#[test]
#[cfg(target_os = "linux")]
fn test_runs_in_temp_directory_by_default() {
    let mut session = TuiSession::spawn_with_config(
        24,
        80,
        SessionConfig::default()
            // Don't include the values that would bypass welcome
            .without_approval_policy()
            .without_sandbox(),
    )
    .expect("Failed to spawn codex");

    session
        .wait_for_text("Powered by Nori AI", TIMEOUT)
        .expect("Prompt did not appear");

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    let contents = session.screen_contents();

    // Should run in /tmp/, not home directory
    assert!(
        contents.contains("/tmp/"),
        "Expected session to run in /tmp/, but got: {}",
        contents
    );

    // Should NOT run in home directory
    assert!(
        !contents.contains("/home/"),
        "Session should not run in home directory, but got: {}",
        contents
    );
    assert_snapshot!(
        "runs_in_temp_directory",
        normalize_for_input_snapshot(contents)
    );
}

#[test]
#[cfg(target_os = "linux")]
fn test_trust_screen_is_skipped_with_default_config() {
    let mut session = TuiSession::spawn(24, 80).expect("Failed to spawn codex");

    // Wait for the prompt to appear (indicated by the chevron character)
    session
        .wait_for_text("›", TIMEOUT)
        .expect("Prompt did not appear");
    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    let contents = session.screen_contents();

    // Should NOT show the trust directory approval screen
    assert!(
        !contents.contains("Since this folder is not version controlled"),
        "Trust screen should be skipped when approval policy is set, but got: {}",
        contents
    );

    // Should show the main prompt directly (skipping onboarding)
    assert!(
        contents.contains("›") && contents.contains("context left"),
        "Should show main prompt with context indicator, got: {}",
        contents
    );
    assert_snapshot!(
        "trust_screen_skipped",
        normalize_for_input_snapshot(contents)
    );
}

#[test]
fn test_startup_shows_nori_banner() {
    // This test verifies the Nori session header appears on startup
    // with the expected branding elements
    let mut session = TuiSession::spawn(24, 80).expect("Failed to spawn codex");

    // Wait for the Nori branding to appear (the "Powered by Nori AI" line)
    session
        .wait_for_text("Powered by Nori AI", TIMEOUT)
        .expect("Nori branding did not appear");
    std::thread::sleep(TIMEOUT_INPUT);

    let contents = session.screen_contents();

    // Verify Nori branding elements are present
    // The ASCII art banner uses special characters like |_| and \_ to spell NORI
    // so we check for the unique pattern from the first line of the banner
    assert!(
        contents.contains("|_) || |"),
        "Expected NORI ASCII banner, but got: {}",
        contents
    );
    assert!(
        contents.contains("Powered by Nori AI"),
        "Expected 'Powered by Nori AI' text, but got: {}",
        contents
    );
    assert!(
        contents.contains("npx nori-ai install"),
        "Expected install instructions, but got: {}",
        contents
    );

    assert_snapshot!(
        "startup_shows_nori_banner",
        normalize_for_input_snapshot(session.screen_contents())
    );
}

#[test]
fn test_poll_does_not_block_when_no_data() {
    // RED phase: This test verifies that poll() returns quickly when no data is available,
    // proving the PTY reader is in non-blocking mode
    let mut session = TuiSession::spawn(24, 80).expect("Failed to spawn codex");

    // Wait for initial startup to complete
    session
        .wait_for_text("›", TIMEOUT)
        .expect("Initial startup failed");

    // Wait for screen to stabilize - keep polling until contents don't change
    let mut prev_contents = String::new();
    for _ in 0..20 {
        session.poll().expect("Poll failed during stabilization");
        std::thread::sleep(Duration::from_millis(100));
        let contents = session.screen_contents();
        if contents == prev_contents {
            // No change for 100ms, screen is stable
            break;
        }
        prev_contents = contents;
    }

    // Now codex is truly waiting for input, no more data will come
    // Poll should return immediately without blocking
    let start = Instant::now();
    session.poll().expect("Poll failed");
    let elapsed = start.elapsed();

    // Assert poll() completed in < 50ms (proves non-blocking)
    // If blocking, would wait indefinitely and this would timeout
    assert!(
        elapsed < Duration::from_millis(50),
        "poll() took {:?}, expected < 50ms. Reader appears to be blocking!",
        elapsed
    );
}

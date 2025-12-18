use insta::assert_snapshot;
use std::time::Duration;
use std::time::Instant;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
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
    .expect("Failed to spawn");

    // When acp.allow_http_fallback=false (default) and the model is not registered as an ACP agent,
    // the TUI should show an error immediately at startup (not after prompt submission).
    // The error is shown before the TUI even renders the shortcuts prompt.
    session
        .wait_for_text("not registered as an ACP agent", TIMEOUT)
        .unwrap();

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);
    let contents = session.screen_contents();

    assert!(
        contents.contains("Model 'nonexistent' is not registered as an ACP agent. Set acp.allow_http_fallback = true to allow HTTP providers."),
        "Missing the required error message, screen contents: {}",
        contents
    );
    // assert_snapshot!(
    //     "startup_error_unregistered_model",
    //     normalize_for_input_snapshot(contents)
    // );
}

#[test]
#[cfg(target_os = "linux")]
fn test_startup_shows_welcome() {
    let mut session = TuiSession::spawn_with_config(
        24,
        80,
        SessionConfig::default()
            // Don't include the values that would bypass welcome
            .without_approval_policy()
            .without_sandbox()
            .with_config_toml(""),
    )
    .expect("Failed to spawn");

    session
        .wait_for_text("Welcome to Nori", TIMEOUT)
        .expect("Prompt did not appear");
    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    let contents = session.screen_contents();
    assert!(contents.contains("Welcome to Nori, your AI coding assistant"));
    assert_snapshot!(
        "startup_shows_welcome",
        normalize_for_input_snapshot(contents)
    );
}

#[test]
#[cfg(target_os = "linux")]
fn test_startup_with_dimensions() {
    let mut session = TuiSession::spawn_with_config(
        10,
        120,
        SessionConfig::default()
            // Don't include the values that would bypass welcome
            .without_approval_policy()
            .without_sandbox(),
    )
    .expect("Failed to spawn");

    session
        .wait_for_text("Powered by Nori AI", TIMEOUT)
        .expect("Prompt did not appear");
    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    // Verify terminal size is respected
    let contents = session.screen_contents();
    assert!(contents.lines().count() <= 10);
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
    .expect("Failed to spawn");

    session
        .wait_for(
            |contents| {
                contents.contains("Powered by Nori AI") || contents.contains("Welcome to Nori")
            },
            TIMEOUT,
        )
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
}

#[test]
#[cfg(target_os = "linux")]
fn test_trust_screen_is_skipped_with_default_config() {
    let mut session = TuiSession::spawn(24, 80).expect("Failed to spawn");

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
}

#[cfg(target_os = "linux")]
#[test]
fn test_startup_shows_nori_banner() {
    // This test verifies the Nori session header appears on startup
    // with the expected branding elements when nori-ai is NOT installed

    use tui_pty_e2e::normalize_for_snapshot;
    let mut session = TuiSession::spawn_with_config(
        24,
        80,
        SessionConfig::default().with_excluded_binary("nori-ai"),
    )
    .expect("Failed to spawn");

    // Wait for the Nori branding to appear (the "Powered by Nori AI" line)
    session
        .wait_for_text("Powered by Nori AI", TIMEOUT)
        .expect("Nori branding did not appear");

    let contents = session.screen_contents();

    // Verify Nori branding elements are present
    // The ASCII art banner uses special characters like |_| and \_ to spell NORI
    // so we check for the unique pattern from the first line of the banner
    assert!(
        contents.contains("Nori v0"),
        "Expected NORI header, but got: {}",
        contents
    );
    assert!(
        contents.contains("Powered by Nori AI"),
        "Expected 'Powered by Nori AI' text, but got: {}",
        contents
    );
    // When nori-ai is NOT installed, show the npx install instructions
    assert!(
        contents.contains("npx nori-ai install"),
        "Expected install instructions when nori-ai not installed, but got: {}",
        contents
    );

    let lines = contents.lines();
    assert_snapshot!(
        "startup_shows_nori_banner",
        normalize_for_snapshot(lines.collect::<Vec<&str>>()[1..8].join("\n"))
    );
}

#[cfg(target_os = "linux")]
#[test]
fn test_startup_hides_install_hint_when_nori_installed() {
    // This test verifies that when nori-ai IS installed (available in PATH),
    // the install instructions are NOT shown
    use std::os::unix::fs::PermissionsExt;

    // Create a temp directory for our mock nori-ai binary
    let mock_bin_dir = tempfile::tempdir().expect("Failed to create temp dir for mock binary");

    // Create a mock nori-ai executable (just needs to exist and be executable)
    let mock_nori = mock_bin_dir.path().join("nori-ai");
    std::fs::write(&mock_nori, "#!/bin/sh\nexit 0\n").expect("Failed to write mock nori-ai");
    std::fs::set_permissions(&mock_nori, std::fs::Permissions::from_mode(0o755))
        .expect("Failed to set permissions on mock nori-ai");

    let mut session = TuiSession::spawn_with_config(
        24,
        80,
        SessionConfig::default().with_extra_path(mock_bin_dir.path().to_path_buf()),
    )
    .expect("Failed to spawn codex");

    // Wait for the Nori branding to appear
    session
        .wait_for_text("Powered by Nori AI", TIMEOUT)
        .expect("Nori branding did not appear");

    let contents = session.screen_contents();

    // Verify Nori branding is present
    assert!(
        contents.contains("Powered by Nori AI"),
        "Expected 'Powered by Nori AI' text, but got: {}",
        contents
    );

    // When nori-ai IS installed, the install instructions should NOT be shown
    assert!(
        !contents.contains("npx nori-ai install"),
        "Install instructions should NOT be shown when nori-ai is installed, but got: {}",
        contents
    );
}

#[test]
fn test_poll_does_not_block_when_no_data() {
    // RED phase: This test verifies that poll() returns quickly when no data is available,
    // proving the PTY reader is in non-blocking mode
    let mut session = TuiSession::spawn(24, 80).expect("Failed to spawn");

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

    // Now nori is truly waiting for input, no more data will come
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

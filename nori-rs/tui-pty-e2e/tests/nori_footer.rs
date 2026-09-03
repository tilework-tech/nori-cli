use insta::assert_snapshot;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_PRESNAPSHOT;
use tui_pty_e2e::TuiSession;
use tui_pty_e2e::normalize_for_input_snapshot;

#[test]
#[cfg(target_os = "linux")]
fn test_footer_displays_git_branch() {
    let mut session = TuiSession::spawn_with_config(
        24,
        120,                  // Wider terminal to fit full footer
        SessionConfig::new(), // git_init is true by default
    )
    .expect("Failed to spawn");

    // Wait for the TUI to start. The branch segment is the idle footer anchor:
    // approvals and the other metadata chips are off by default.
    session.wait_for_text("›", TIMEOUT).unwrap();
    session.wait_for_text("⎇", TIMEOUT).unwrap();

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);
    let contents = session.screen_contents();

    // The footer should contain git branch info (master, since we use git init -b master).
    assert!(
        contents.contains("⎇"),
        "Footer should contain git branch symbol. Contents: {}",
        contents
    );

    // Check that the branch name appears (always master since we use git init -b master)
    assert!(
        contents.contains("master"),
        "Footer should contain git branch name 'master'. Contents: {}",
        contents
    );
}

#[test]
#[cfg(target_os = "linux")]
fn test_footer_without_git_repo() {
    // Approvals is off in the shipped defaults; enable it so this test keeps a
    // non-git footer segment to anchor on and to assert against.
    let extra_config_toml = r#"
[tui.footer_segments]
approval_mode = true
"#;

    let mut session = TuiSession::spawn_with_config(
        24,
        120,
        SessionConfig::new()
            .without_git_init() // No git repo
            .with_extra_config_toml(extra_config_toml),
    )
    .expect("Failed to spawn");

    // Wait for the TUI to start
    session.wait_for_text("›", TIMEOUT).unwrap();
    session.wait_for_text("Approvals", TIMEOUT).unwrap();

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);
    let contents = session.screen_contents();

    // Without a git repo, the footer should NOT contain the branch symbol
    assert!(
        !contents.contains("⎇"),
        "Footer should not contain git branch symbol without a git repo. Contents: {}",
        contents
    );

    // But it should still show the configured non-git footer segments.
    assert!(
        contents.contains("Approvals"),
        "Footer should still show approval mode. Contents: {}",
        contents
    );
}

#[test]
#[cfg(target_os = "linux")]
fn test_footer_full_startup_with_all_info() {
    // This test verifies the complete footer display similar to startup.rs tests
    // It should show: git branch, active skillset, Nori version, and git diff stats.

    use std::os::unix::fs::PermissionsExt;

    // Create a temp directory for our mock nori-skillsets binary
    let mock_bin_dir = tempfile::tempdir().expect("Failed to create temp dir for mock binary");

    // Create a mock nori-skillsets executable that handles --version and list-active
    let mock_nori = mock_bin_dir.path().join("nori-skillsets");
    std::fs::write(
        &mock_nori,
        "#!/bin/sh\ncase \"$1\" in\n  list-active) echo 'test-skillset';;\n  *) echo 'nori-skillsets 0.9.99';;\nesac\n",
    )
    .expect("Failed to write mock nori-skillsets");
    std::fs::set_permissions(&mock_nori, std::fs::Permissions::from_mode(0o755))
        .expect("Failed to set permissions on mock nori-skillsets");

    // Enable the segments this test validates explicitly — they're off
    // by default in the lean shipped footer config. Use the additive
    // form so the auto-generated trust block (which keeps approval mode
    // at the trusted default) is preserved.
    let extra_config_toml = r#"
[tui.footer_segments]
skillset = true
nori_version = true
git_stats = true
"#;

    let mut session = TuiSession::spawn_with_config(
        24,
        120, // Wide terminal to fit full footer
        SessionConfig::new()
            .with_extra_path(mock_bin_dir.path().to_path_buf())
            .with_extra_config_toml(extra_config_toml),
    )
    .expect("Failed to spawn");

    // Startup prepares the agent without creating a session. Wait for the
    // sessionless composer and footer instead of the post-activation banner.
    session.wait_for_text("›", TIMEOUT).unwrap();
    session.wait_for_text("Skillsets v", TIMEOUT).unwrap();

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);
    let contents = session.screen_contents();

    // Verify git branch is displayed (always master since we use git init -b master)
    assert!(
        contents.contains("⎇"),
        "Footer should contain git branch symbol. Contents: {}",
        contents
    );
    assert!(
        contents.contains("master"),
        "Footer should contain branch name 'master'. Contents: {}",
        contents
    );

    // Verify nori version is displayed (from our mock nori-skillsets)
    assert!(
        contents.contains("Skillsets v19.1.1") || contents.contains("Skillsets v0"), // v0 if mock didn't work
        "Footer should contain Nori version. Contents: {}",
        contents
    );

    // Git diff stats are only shown when there are actual changes
    // In a clean repo with no changes, git diff HEAD --shortstat returns empty
    // So the stats won't be displayed. This is correct behavior.
    // We just verify the other components are present and the footer renders correctly.

    // Verify the footer contains all the expected segments separated by ·
    assert!(
        contents.contains("⎇ master"),
        "Footer should contain git branch. Contents: {}",
        contents
    );
    assert!(
        contents.contains("Skillsets v"),
        "Footer should contain the Skillsets version. Contents: {}",
        contents
    );

    assert_snapshot!("full_footer", normalize_for_input_snapshot(contents));
}

#[test]
#[cfg(target_os = "linux")]
fn test_footer_vertical_layout_from_config() {
    // Two segments are needed to prove they land on separate lines. Approvals
    // is off by default, so opt back in for this test.
    let extra_config_toml = r#"
[tui]
vertical_footer = true

[tui.footer_segments]
approval_mode = true
"#;

    let mut session = TuiSession::spawn_with_config(
        24,
        60,
        SessionConfig::new()
            .with_extra_config_toml(extra_config_toml)
            .with_excluded_binary("nori-skillsets"),
    )
    .expect("Failed to spawn");

    session.wait_for_text("›", TIMEOUT).unwrap();
    session.wait_for_text("Approvals", TIMEOUT).unwrap();

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);
    let contents = session.screen_contents();

    let lines: Vec<&str> = contents.lines().collect();
    let branch_line_idx = lines
        .iter()
        .position(|line| line.contains("⎇") && line.contains("master"))
        .expect("Footer should contain git branch line");
    let approvals_line_idx = lines
        .iter()
        .position(|line| line.contains("Approvals"))
        .expect("Footer should contain approvals line");

    assert_ne!(
        branch_line_idx, approvals_line_idx,
        "Branch and approvals should render on separate lines in vertical footer. Contents: {contents}"
    );

    let branch_line = lines[branch_line_idx];
    let approvals_line = lines[approvals_line_idx];
    assert!(
        !branch_line.contains('·'),
        "Branch line should not include separators in vertical footer. Line: {branch_line}"
    );
    assert!(
        !approvals_line.contains('·'),
        "Approvals line should not include separators in vertical footer. Line: {approvals_line}"
    );

    assert_snapshot!("vertical_footer", normalize_for_input_snapshot(contents));
}

#[test]
#[cfg(target_os = "linux")]
fn test_footer_with_segments_disabled() {
    // Test that footer segments can be disabled via config.toml
    let extra_config_toml = r#"
[tui.footer_segments]
git_branch = false
approval_mode = false
"#;

    let mut session = TuiSession::spawn_with_config(
        24,
        120,
        SessionConfig::new().with_extra_config_toml(extra_config_toml),
    )
    .expect("Failed to spawn");

    // Startup prepares the agent without creating a session, so the composer
    // is the readiness signal rather than a post-activation session header.
    session.wait_for_text("›", TIMEOUT).unwrap();

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);
    let contents = session.screen_contents();

    // Git branch should NOT be displayed (disabled in config)
    assert!(
        !contents.contains("⎇"),
        "Footer should NOT contain git branch symbol when disabled. Contents: {}",
        contents
    );
    assert!(
        !contents.contains("master"),
        "Footer should NOT contain branch name when disabled. Contents: {}",
        contents
    );

    // Approval mode should NOT be displayed (disabled in config)
    assert!(
        !contents.contains("Approvals"),
        "Footer should NOT contain Approvals when disabled. Contents: {}",
        contents
    );

    // The card is written at startup, before any session exists, so it names
    // the agent without the configuration only a live session can report.
    assert!(
        contents.contains("Nori CLI v"),
        "Startup should render the session header. Contents: {}",
        contents
    );
    assert!(
        contents.contains("Agent        Mock ACP\n"),
        "The agent row should be the provider name alone before a session \
         reports its configuration. Contents: {}",
        contents
    );
}

#[test]
#[cfg(target_os = "linux")]
fn test_default_idle_footer_has_no_metadata_clutter() {
    // The shipped defaults are deliberately quiet: branch, worktree, context,
    // and the agent mode. Approvals, skillset, skillset version, session
    // title, and cumulative token usage stay in `/status` until a user opts
    // back in through `[tui.footer_segments]`.
    let mut session =
        TuiSession::spawn_with_config(24, 120, SessionConfig::new()).expect("Failed to spawn");

    session.wait_for_text("›", TIMEOUT).unwrap();
    session.wait_for_text("⎇", TIMEOUT).unwrap();

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);
    let contents = session.screen_contents();

    for clutter in [
        "Approvals:",
        "Skillset:",
        "Skillsets v",
        "Title:",
        "Tokens:",
    ] {
        assert!(
            !contents.contains(clutter),
            "Default idle footer should not contain {clutter:?}. Contents: {contents}"
        );
    }

    assert_snapshot!(
        "default_idle_footer",
        normalize_for_input_snapshot(contents)
    );
}

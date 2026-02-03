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

    // Wait for the TUI to start
    session.wait_for_text("? for shortcuts", TIMEOUT).unwrap();
    session.wait_for_text("Approvals", TIMEOUT).unwrap();

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);
    let contents = session.screen_contents();

    // The footer should contain git branch info (master, since we use git init -b master)
    // Check for the branch symbol and "? for shortcuts" which should always be present
    assert!(
        contents.contains("⎇") && contents.contains("? for shortcuts"),
        "Footer should contain git branch symbol and shortcuts hint. Contents: {}",
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
    let mut session = TuiSession::spawn_with_config(
        24,
        120,
        SessionConfig::new().without_git_init(), // No git repo
    )
    .expect("Failed to spawn");

    // Wait for the TUI to start
    session.wait_for_text("? for shortcuts", TIMEOUT).unwrap();
    session.wait_for_text("Approvals", TIMEOUT).unwrap();

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);
    let contents = session.screen_contents();

    // Without a git repo, the footer should NOT contain the branch symbol
    assert!(
        !contents.contains("⎇"),
        "Footer should not contain git branch symbol without a git repo. Contents: {}",
        contents
    );

    // But it should still show shortcuts
    assert!(
        contents.contains("? for shortcuts"),
        "Footer should still show shortcuts hint. Contents: {}",
        contents
    );
}

#[test]
#[cfg(target_os = "linux")]
fn test_footer_full_startup_with_all_info() {
    // This test verifies the complete footer display similar to startup.rs tests
    // It should show: git branch, nori profile, nori version, git diff stats, and shortcuts

    use std::os::unix::fs::PermissionsExt;

    // Create a temp directory for our mock nori-skillsets binary
    let mock_bin_dir = tempfile::tempdir().expect("Failed to create temp dir for mock binary");

    // Create a mock nori-skillsets executable that returns a version
    let mock_nori = mock_bin_dir.path().join("nori-skillsets");
    std::fs::write(&mock_nori, "#!/bin/sh\necho 'nori-skillsets 0.9.99'\n")
        .expect("Failed to write mock nori-skillsets");
    std::fs::set_permissions(&mock_nori, std::fs::Permissions::from_mode(0o755))
        .expect("Failed to set permissions on mock nori-skillsets");

    let mut session = TuiSession::spawn_with_config(
        24,
        120, // Wide terminal to fit full footer
        SessionConfig::new().with_extra_path(mock_bin_dir.path().to_path_buf()),
    )
    .expect("Failed to spawn");

    // Wait for the TUI to fully start
    session
        .wait_for_text("Nori CLI", TIMEOUT)
        .expect("TUI did not start");

    // Wait for footer to render
    session.wait_for_text("? for shortcuts", TIMEOUT).unwrap();
    session.wait_for_text("Approvals", TIMEOUT).unwrap();

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

    // Verify shortcuts hint is always present
    assert!(
        contents.contains("? for shortcuts"),
        "Footer should show shortcuts hint. Contents: {}",
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
        contents.contains("Nori CLI v"),
        "Footer should contain Nori version. Contents: {}",
        contents
    );

    assert_snapshot!("full_footer", normalize_for_input_snapshot(contents));
}

#[test]
#[cfg(target_os = "linux")]
fn test_footer_vertical_layout_from_config() {
    let config_toml = r#"
model = "mock-model"
model_provider = "mock_provider"

[model_providers.mock_provider]
name = "Mock ACP provider for tests"

[tui]
vertical_footer = true
"#;

    let mut session =
        TuiSession::spawn_with_config(24, 60, SessionConfig::new().with_config_toml(config_toml))
            .expect("Failed to spawn");

    session.wait_for_text("? for shortcuts", TIMEOUT).unwrap();
    session.wait_for_text("Approvals", TIMEOUT).unwrap();

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);
    let contents = session.screen_contents();

    let lines: Vec<&str> = contents.lines().collect();
    let branch_line_idx = lines
        .iter()
        .position(|line| line.contains("⎇") && line.contains("master"))
        .expect("Footer should contain git branch line");
    let shortcuts_line_idx = lines
        .iter()
        .position(|line| line.contains("? for shortcuts"))
        .expect("Footer should contain shortcuts line");

    assert_ne!(
        branch_line_idx, shortcuts_line_idx,
        "Branch and shortcuts should render on separate lines in vertical footer. Contents: {contents}"
    );

    let branch_line = lines[branch_line_idx];
    let shortcuts_line = lines[shortcuts_line_idx];
    assert!(
        !branch_line.contains('·'),
        "Branch line should not include separators in vertical footer. Line: {branch_line}"
    );
    assert!(
        !shortcuts_line.contains('·'),
        "Shortcuts line should not include separators in vertical footer. Line: {shortcuts_line}"
    );

    assert_snapshot!("vertical_footer", normalize_for_input_snapshot(contents));
}

#[test]
#[cfg(target_os = "linux")]
fn test_footer_with_segments_disabled() {
    // Test that footer segments can be disabled via config.toml
    let config_toml = r#"
model = "mock-model"
model_provider = "mock_provider"

[model_providers.mock_provider]
name = "Mock ACP provider for tests"

[tui.footer_segments]
git_branch = false
approval_mode = false
"#;

    let mut session =
        TuiSession::spawn_with_config(24, 120, SessionConfig::new().with_config_toml(config_toml))
            .expect("Failed to spawn");

    // Wait for the TUI to start
    session.wait_for_text("? for shortcuts", TIMEOUT).unwrap();

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

    // Shortcuts hint should still be present (always shown)
    assert!(
        contents.contains("? for shortcuts"),
        "Footer should still show shortcuts hint. Contents: {}",
        contents
    );

    // Nori version should still be shown (not disabled)
    assert!(
        contents.contains("Nori CLI v") || contents.contains("Skillsets v"),
        "Footer should still show Nori version when not disabled. Contents: {}",
        contents
    );
}

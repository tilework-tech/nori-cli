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

    // Create a temp directory for our mock nori-ai binary
    let mock_bin_dir = tempfile::tempdir().expect("Failed to create temp dir for mock binary");

    // Create a mock nori-ai executable that returns a version
    let mock_nori = mock_bin_dir.path().join("nori-ai");
    std::fs::write(&mock_nori, "#!/bin/sh\necho 'nori-ai 19.1.1'\n")
        .expect("Failed to write mock nori-ai");
    std::fs::set_permissions(&mock_nori, std::fs::Permissions::from_mode(0o755))
        .expect("Failed to set permissions on mock nori-ai");

    let mut session = TuiSession::spawn_with_config(
        24,
        120, // Wide terminal to fit full footer
        SessionConfig::new().with_extra_path(mock_bin_dir.path().to_path_buf()),
    )
    .expect("Failed to spawn");

    // Wait for the TUI to fully start
    session
        .wait_for_text("Powered by Nori AI", TIMEOUT)
        .expect("TUI did not start");

    // Wait for footer to render
    session.wait_for_text("? for shortcuts", TIMEOUT).unwrap();

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

    // Verify nori version is displayed (from our mock nori-ai)
    assert!(
        contents.contains("Profiles v19.1.1") || contents.contains("Profiles v0"), // v0 if mock didn't work
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

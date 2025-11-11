use nori_cli::backends;
use nori_cli::backends::claude::ClaudeBackend;
use nori_cli::backends::AgentBackend;

#[test]
fn test_backend_availability_check_installed() {
    // sh is always present on Unix systems
    assert!(
        backends::is_available("sh"),
        "sh should be detected as available"
    );
}

#[test]
fn test_backend_availability_check_missing() {
    // Extremely unlikely to exist
    assert!(
        !backends::is_available("nonexistent-command-xyz-12345"),
        "nonexistent command should not be detected as available"
    );
}

#[test]
fn test_claude_backend_provides_install_info() {
    let backend = ClaudeBackend::new();
    assert_eq!(backend.command_name(), "claude");
    assert!(
        backend.install_url().contains("claude.com")
            || backend.install_url().contains("code.claude.com"),
        "Install URL should point to Claude website"
    );
}

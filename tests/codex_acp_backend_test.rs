use nori_cli::backends::AgentBackend;
use nori_cli::backends::codex_acp::CodexAcpBackend;

#[test]
fn test_codex_acp_backend_creation() {
    let backend = CodexAcpBackend::new();
    assert_eq!(backend.name(), "Codex ACP");
}

#[test]
fn test_codex_acp_command_name_returns_runtime_executable() {
    let backend = CodexAcpBackend::new();
    let cmd = backend.command_name();
    // Should be either bunx, npx, or a fallback
    assert!(
        cmd == "bunx" || cmd == "npx",
        "Command should be bunx or npx, got: {}",
        cmd
    );
}

#[test]
fn test_codex_acp_install_command_provides_npm_install() {
    let backend = CodexAcpBackend::new();
    let install_cmd = backend.install_command();
    assert!(install_cmd.is_some());

    let cmd = install_cmd.unwrap();
    assert_eq!(cmd[0], "npm");
    assert_eq!(cmd[1], "install");
    assert_eq!(cmd[2], "-g");
    assert_eq!(cmd[3], "@zed-industries/codex-acp");
}

#[test]
fn test_codex_acp_install_url_points_to_npm_package() {
    let backend = CodexAcpBackend::new();
    assert!(backend.install_url().contains("npmjs.com"));
    assert!(backend.install_url().contains("codex-acp"));
}

use nori_cli::backends::AgentBackend;
use nori_cli::backends::gemini_acp::GeminiAcpBackend;

#[test]
fn test_gemini_acp_backend_creation() {
    let backend = GeminiAcpBackend::new();
    assert_eq!(backend.name(), "Gemini ACP");
}

#[test]
fn test_gemini_acp_command_name_returns_runtime_executable() {
    let backend = GeminiAcpBackend::new();
    let cmd = backend.command_name();
    // Should be either bunx, npx, or a fallback
    assert!(
        cmd == "bunx" || cmd == "npx",
        "Command should be bunx or npx, got: {cmd}"
    );
}

#[test]
fn test_gemini_acp_install_command_provides_npm_install() {
    let backend = GeminiAcpBackend::new();
    let install_cmd = backend.install_command();
    assert!(install_cmd.is_some());

    let cmd = install_cmd.unwrap();
    assert_eq!(cmd[0], "npm");
    assert_eq!(cmd[1], "install");
    assert_eq!(cmd[2], "-g");
    assert_eq!(cmd[3], "@google/gemini-cli");
}

#[test]
fn test_gemini_acp_install_url_points_to_npm_package() {
    let backend = GeminiAcpBackend::new();
    assert!(backend.install_url().contains("npmjs.com"));
    assert!(backend.install_url().contains("gemini-cli"));
}

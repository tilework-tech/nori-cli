use clap::Parser;
use nori_cli::cli::Cli;

#[test]
fn test_agent_long_flag() {
    let cli = Cli::try_parse_from(&["prog", "--agent", "claude"]).unwrap();
    assert_eq!(cli.agent, Some("claude".to_string()));
    assert_eq!(cli.message, None);
}

#[test]
fn test_agent_short_flag() {
    let cli = Cli::try_parse_from(&["prog", "-a", "codex"]).unwrap();
    assert_eq!(cli.agent, Some("codex".to_string()));
    assert_eq!(cli.message, None);
}

#[test]
fn test_message_only() {
    let cli = Cli::try_parse_from(&["prog", "Hello world"]).unwrap();
    assert_eq!(cli.agent, None);
    assert_eq!(cli.message, Some("Hello world".to_string()));
}

#[test]
fn test_agent_and_message() {
    let cli = Cli::try_parse_from(&["prog", "--agent", "mock", "Test message"]).unwrap();
    assert_eq!(cli.agent, Some("mock".to_string()));
    assert_eq!(cli.message, Some("Test message".to_string()));
}

#[test]
fn test_no_arguments() {
    let cli = Cli::try_parse_from(&["prog"]).unwrap();
    assert_eq!(cli.agent, None);
    assert_eq!(cli.message, None);
}

// Agent name mapping tests
use nori_cli::cli::agent_name_to_index;

#[test]
fn test_agent_name_claude() {
    assert_eq!(agent_name_to_index("claude"), Some(0));
}

#[test]
fn test_agent_name_codex() {
    assert_eq!(agent_name_to_index("codex"), Some(1));
}

#[test]
fn test_agent_name_claudecode() {
    assert_eq!(agent_name_to_index("claudecode"), Some(2));
}

#[test]
fn test_agent_name_mock() {
    assert_eq!(agent_name_to_index("mock"), Some(3));
}

#[test]
fn test_agent_name_case_insensitive() {
    assert_eq!(agent_name_to_index("Claude"), Some(0));
    assert_eq!(agent_name_to_index("CODEX"), Some(1));
}

#[test]
fn test_agent_name_invalid() {
    assert_eq!(agent_name_to_index("invalid"), None);
    assert_eq!(agent_name_to_index(""), None);
}

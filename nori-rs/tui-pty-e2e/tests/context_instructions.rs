use pretty_assertions::assert_eq;
use std::path::Path;
use std::time::Duration;
use std::time::Instant;

use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TuiSession;

/// The final rendered inventory row for this file, excluding earlier snapshots.
fn instruction_tokens(screen: &str, path: &Path) -> Option<i64> {
    let suffix = path.to_string_lossy();
    screen.lines().rev().find_map(|line| {
        let mut fields = line.split_whitespace();
        let label = fields.next()?;
        if !label.ends_with(suffix.as_ref()) {
            return None;
        }
        fields
            .next()?
            .strip_prefix('~')?
            .replace(',', "")
            .parse()
            .ok()
    })
}

#[test]
fn repeated_context_with_real_claude_instructions_is_prompt_and_reads_edits() {
    let user_home = tempfile::tempdir().expect("isolated instruction home");
    let mock = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(format!("mock_acp_agent{}", std::env::consts::EXE_SUFFIX));
    let command = serde_json::to_string(&mock.to_string_lossy()).unwrap();
    let config = SessionConfig::new()
        .with_real_instruction_files()
        .with_agent("claude-context-fixture".into())
        .with_agent_env("HOME", user_home.path().to_string_lossy())
        .with_extra_config_toml(format!(
            r#"
[[agents]]
name = "Claude context fixture"
slug = "claude-context-fixture"
[agents.distribution.local]
command = {command}
"#
        ));
    let mut session = TuiSession::spawn_with_config(70, 220, config).expect("start real CLI");
    session.wait_for_text("›", TIMEOUT).expect("composer ready");
    let cwd = session.nori_home_path().expect("isolated project");
    let paths = [
        cwd.join("CLAUDE.md"),
        cwd.join("CLAUDE.local.md"),
        cwd.join(".claude/CLAUDE.md"),
    ];
    std::fs::create_dir_all(cwd.join(".claude")).expect("create hidden instruction directory");
    let contents = "Use the component library. Keep interactions responsive.\n".repeat(100);
    for path in &paths {
        std::fs::write(path, &contents).expect("write real instruction file");
    }

    // Allow one cold tokenizer initialization before enforcing interactive latency.
    session
        .submit_input("/context")
        .expect("warm instruction discovery");
    session
        .wait_for(
            |screen| {
                paths
                    .iter()
                    .all(|path| instruction_tokens(screen, path).is_some_and(|tokens| tokens > 0))
            },
            TIMEOUT,
        )
        .expect("three real instruction counts");
    let screen = session.screen_contents();
    let initial: Vec<_> = paths
        .iter()
        .map(|path| instruction_tokens(&screen, path).unwrap())
        .collect();
    let mut previous = initial[1];
    let budget = Duration::from_secs(1);
    for multiplier in [2, 3] {
        std::fs::write(&paths[1], contents.repeat(multiplier)).expect("edit instruction contents");
        session
            .type_input("/context")
            .expect("compose repeated command");
        let started = Instant::now();
        session.send_key(Key::Enter).expect("request fresh context");
        session
            .wait_for(
                |screen| {
                    instruction_tokens(screen, &paths[1]).is_some_and(|tokens| tokens > previous)
                },
                budget,
            )
            .expect("updated instruction count within the one-second warm budget");
        let elapsed = started.elapsed();
        assert!(elapsed < budget, "warm /context took {elapsed:?}");
        let screen = session.screen_contents();
        previous = instruction_tokens(&screen, &paths[1]).unwrap();
        assert_eq!(instruction_tokens(&screen, &paths[0]), Some(initial[0]));
        assert_eq!(instruction_tokens(&screen, &paths[2]), Some(initial[2]));
        eprintln!(
            "warm /context after edit {multiplier}: {elapsed:?}, local instruction tokens: {previous}"
        );
    }
}

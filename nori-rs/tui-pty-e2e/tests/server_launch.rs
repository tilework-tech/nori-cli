//! Exercise the same registered ACP launch from delegation and the picker.
#![cfg(unix)]

use std::fs;
use std::path::PathBuf;

use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TuiSession;

#[test]
fn delegated_and_picked_servers_share_the_recipe_and_isolate_the_next_agent()
-> Result<(), Box<dyn std::error::Error>> {
    let runtime = tempfile::tempdir()?;
    let wrapper = runtime.path().join("record args.sh");
    fs::write(
        &wrapper,
        r#"
[ "$#" = 3 ] && [ "$1" = "prefix arg" ] && [ "$3" = "server arg" ] || exit 64
printf '%s\n' "$EXAMPLE_SCOPE" >> "$NORI_AGENT_RUNTIME_DIR/launches" || exit 65
shift
exec "$@"
"#,
    )?;
    let mock = std::env::var_os("MOCK_ACP_AGENT_BIN")
        .map(PathBuf::from)
        .unwrap_or(
            std::env::current_exe()?
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("mock_acp_agent"),
        );
    let definition = format!(
        r#"
[[agents]]
name = "Lab Agent"
slug = "lab-agent"
[agents.distribution.local]
command = {command:?}
args = ["server arg"]
wrapper = ["/bin/sh", {wrapper:?}, "prefix arg"]
env = {{ EXAMPLE_SCOPE = "child fixture" }}
runtime_dir = {runtime:?}
"#,
        command = mock.to_str().unwrap(),
        wrapper = wrapper.to_str().unwrap(),
        runtime = runtime.path().to_str().unwrap()
    );
    let config = SessionConfig::new()
        .with_agent_env("MOCK_AGENT_ECHO_ENV", "EXAMPLE_SCOPE")
        .with_agent_env("EXAMPLE_SCOPE", "parent fixture")
        .with_extra_config_toml(definition);
    let mut session = TuiSession::spawn_with_config(40, 120, config)?;
    session.wait_for_text("Mock Default Model", TIMEOUT)?;
    session.submit_input("/delegate lab-agent synthetic turn")?;
    session.wait_for_text("Yes, start it", TIMEOUT)?;
    session.wait_out_answer_grace();
    session.send_key(Key::Char('1'))?;
    session.wait_for_text("result from lab-agent", TIMEOUT)?;
    assert_eq!(
        fs::read_to_string(runtime.path().join("launches"))?,
        "child fixture\n"
    );

    for (key, row, label, scope) in [
        ("\x1b[F", "▏  Lab Agent", "Lab Agent", "child fixture"),
        ("\x1b[H", "▏1. Mock ACP", "Mock ACP", "parent fixture"),
    ] {
        session.submit_input("/agent")?;
        session.wait_for_text("Select agent", TIMEOUT)?;
        session.send_str(key)?;
        session.wait_for_text(row, TIMEOUT)?;
        session.send_key(Key::Enter)?;
        session.wait_for_text("Start a new session", TIMEOUT)?;
        session.send_key(Key::Enter)?;
        session.wait_for_text(
            &format!("Started new conversation with agent: {label}"),
            TIMEOUT,
        )?;
        session.submit_input("show the synthetic setting")?;
        session.wait_for_text(&format!("ENV:EXAMPLE_SCOPE={scope}"), TIMEOUT)?;
    }
    assert_eq!(
        fs::read_to_string(runtime.path().join("launches"))?,
        "child fixture\nchild fixture\n"
    );
    Ok(())
}

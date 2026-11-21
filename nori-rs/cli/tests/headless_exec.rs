use std::io;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::process::Child;
use std::process::ChildStdin;
use std::process::Command as StdCommand;
use std::process::Stdio;
use std::sync::mpsc;
use std::time::Duration;

use assert_cmd::Command;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;

fn mock_agent_bin() -> io::Result<String> {
    std::env::var("MOCK_ACP_AGENT_BIN").map_err(|_| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "MOCK_ACP_AGENT_BIN must point to the built mock_acp_agent binary",
        )
    })
}

fn nori_command(nori_home: &TempDir) -> Result<Command, Box<dyn std::error::Error>> {
    let mut command = Command::cargo_bin("nori")?;
    command
        .env("NORI_HOME", nori_home.path())
        .env("MOCK_ACP_AGENT_BIN", mock_agent_bin()?)
        .args(["--agent", "mock-model"]);
    Ok(command)
}

#[cfg(unix)]
#[test]
fn exec_launches_hermes_and_cursor_by_cli_alias() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;

    for (alias, executable) in [("hermes-agent", "hermes"), ("cursor-agent", "cursor-agent")] {
        let home = TempDir::new()?;
        let runner = home.path().join(executable);
        std::fs::write(
            &runner,
            r#"#!/bin/sh
[ "$#" = 1 ] && [ "$1" = acp ] || exit 64
export MOCK_AGENT_ECHO_PROMPT=1
exec "$ALIAS_FIXTURE_BIN"
"#,
        )?;
        std::fs::set_permissions(&runner, std::fs::Permissions::from_mode(0o755))?;
        let output = Command::cargo_bin("nori")?
            .env_clear()
            .current_dir(home.path())
            .env("PATH", home.path())
            .env("HOME", home.path())
            .env("NORI_HOME", home.path())
            .env("NORI_MANAGED_BY_NPM", "1")
            .env("ALIAS_FIXTURE_BIN", mock_agent_bin()?)
            .args(["--agent", alias, "exec", "alias launch fixture"])
            .output()?;
        assert!(
            output.status.success(),
            "{alias}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8(output.stdout)?, "alias launch fixture\n");
    }
    Ok(())
}

#[test]
fn exec_prints_only_the_final_agent_response() -> Result<(), Box<dyn std::error::Error>> {
    let nori_home = TempDir::new()?;
    let output = nori_command(&nori_home)?
        .env("MOCK_AGENT_ECHO_PROMPT", "1")
        .args(["exec", "explain this repository"])
        .output()?;

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout)?,
        "explain this repository\n"
    );
    Ok(())
}

#[test]
fn exec_reads_the_prompt_from_stdin() -> Result<(), Box<dyn std::error::Error>> {
    let nori_home = TempDir::new()?;
    let output = nori_command(&nori_home)?
        .env("MOCK_AGENT_ECHO_PROMPT", "1")
        .arg("exec")
        .write_stdin("prompt from a pipe")
        .output()?;

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout)?, "prompt from a pipe\n");
    Ok(())
}

#[test]
fn exec_composes_the_argument_and_piped_stdin_when_asked() -> Result<(), Box<dyn std::error::Error>>
{
    let nori_home = TempDir::new()?;
    let output = nori_command(&nori_home)?
        .env("MOCK_AGENT_ECHO_PROMPT", "1")
        .args(["exec", "--stdin", "review this"])
        .write_stdin("diff --git a/x b/x\n")
        .output()?;

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout)?,
        "review this\n\ndiff --git a/x b/x\n"
    );
    Ok(())
}

/// A prompt argument on its own must leave stdin completely alone, or `nori
/// exec` inside a `while read` loop swallows the loop's input.
#[test]
fn exec_leaves_stdin_alone_when_given_a_prompt() -> Result<(), Box<dyn std::error::Error>> {
    let nori_home = TempDir::new()?;
    let output = nori_command(&nori_home)?
        .env("MOCK_AGENT_ECHO_PROMPT", "1")
        .args(["exec", "say hi"])
        .write_stdin("this text must not be consumed")
        .output()?;

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout)?, "say hi\n");
    Ok(())
}

#[test]
fn exec_reads_the_prompt_from_the_stdin_sentinel() -> Result<(), Box<dyn std::error::Error>> {
    let nori_home = TempDir::new()?;
    let output = nori_command(&nori_home)?
        .env("MOCK_AGENT_ECHO_PROMPT", "1")
        .args(["exec", "-"])
        .write_stdin("prompt from a pipe")
        .output()?;

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout)?, "prompt from a pipe\n");
    Ok(())
}

#[test]
fn print_rejects_a_subcommand() -> Result<(), Box<dyn std::error::Error>> {
    let nori_home = TempDir::new()?;
    let output = nori_command(&nori_home)?
        .args(["-p", "resume", "--last"])
        .output()?;

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("--print cannot be combined with a subcommand")
    );
    Ok(())
}

#[test]
fn print_rejects_images_it_cannot_attach() -> Result<(), Box<dyn std::error::Error>> {
    let nori_home = TempDir::new()?;
    let output = nori_command(&nori_home)?
        .args(["-p", "--image", "/tmp/example.png", "describe this"])
        .output()?;

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--print does not support --image"));
    Ok(())
}

#[test]
fn print_flag_is_an_alias_for_exec() -> Result<(), Box<dyn std::error::Error>> {
    let nori_home = TempDir::new()?;
    let output = nori_command(&nori_home)?
        .env("MOCK_AGENT_ECHO_PROMPT", "1")
        .args(["-p", "explain this repository"])
        .output()?;

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout)?,
        "explain this repository\n"
    );
    Ok(())
}

#[test]
fn exec_requires_a_prompt_from_some_source() -> Result<(), Box<dyn std::error::Error>> {
    let nori_home = TempDir::new()?;
    let output = nori_command(&nori_home)?
        .arg("exec")
        .write_stdin("   \n")
        .output()?;

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("a prompt argument or piped stdin is required")
    );
    Ok(())
}

#[test]
fn exec_fails_closed_without_waiting_for_an_approval() -> Result<(), Box<dyn std::error::Error>> {
    let nori_home = TempDir::new()?;
    let output = nori_command(&nori_home)?
        .arg("exec")
        .arg("mock:request-permission")
        .timeout(Duration::from_secs(10))
        .output()?;

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("option: reject"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("permission request was rejected"));
    Ok(())
}

#[test]
fn exec_reports_why_the_agent_could_not_start() -> Result<(), Box<dyn std::error::Error>> {
    let nori_home = TempDir::new()?;
    std::fs::write(
        nori_home.path().join("config.toml"),
        "[[token_providers]]\nname = 'subconscious'\npreset = 'subconscious'\nprotocol = 'openai-completions'\nbase_url = 'https://api.subconscious.dev/v1'\nenv_key = 'NORI_EXEC_TEST_SUBC_KEY_UNSET'\nmodel = 'subconscious/glm-5.2'\n\n[agent_token_providers]\npi = 'subconscious'\n",
    )?;
    let output = Command::cargo_bin("nori")?
        .env("NORI_HOME", nori_home.path())
        .env_remove("NORI_EXEC_TEST_SUBC_KEY_UNSET")
        .args(["--agent", "pi", "exec", "hello"])
        .output()?;

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("NORI_EXEC_TEST_SUBC_KEY_UNSET"), "{stderr}");
    Ok(())
}

#[test]
fn exec_reports_agent_prompt_failures_on_stderr() -> Result<(), Box<dyn std::error::Error>> {
    let nori_home = TempDir::new()?;
    let output = nori_command(&nori_home)?
        .env("MOCK_AGENT_PROMPT_FAIL", "1")
        .args(["exec", "fail this prompt"])
        .output()?;

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Mock prompt failure for testing"));
    Ok(())
}

#[cfg(unix)]
mod cursor_startup {
    use super::*;

    fn exec(home: &TempDir) -> StdCommand {
        let mut cmd = StdCommand::new(assert_cmd::cargo::cargo_bin("nori"));
        cmd.env_clear()
            .env("HOME", home.path())
            .env("NORI_HOME", home.path())
            .env("PATH", home.path())
            .current_dir(home.path())
            .args(["exec", "--agent", "cursor", "Describe the sample."]);
        cmd
    }

    #[test]
    fn missing_cursor_executable_explains_installation() {
        let home = TempDir::new().unwrap();
        let output = exec(&home).output().unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(!output.status.success() && output.stdout.is_empty());
        assert!(
            stderr.contains("https://cursor.com/docs/cli/installation"),
            "{stderr}"
        );
        assert!(stderr.contains("PATH"), "{stderr}");
        assert!(!stderr.contains("install it with: install"), "{stderr}");
    }

    #[test]
    fn session_creation_auth_failure_uses_the_selected_agents_hint() {
        let custom = r#"[[agents]]
name = "Example"
slug = "cursor"
auth_hint = "Use the example sign-in command."
[agents.distribution.local]
command = "cursor-agent"
"#;
        for (config, hints) in [
            ("", &["agent login", "CURSOR_API_KEY"][..]),
            (custom, &["Use the example sign-in command."][..]),
        ] {
            let home = TempDir::new().unwrap();
            std::fs::write(home.path().join("config.toml"), config).unwrap();
            std::os::unix::fs::symlink(
                std::fs::canonicalize(mock_agent_bin().unwrap()).unwrap(),
                home.path().join("cursor-agent"),
            )
            .unwrap();
            let output = exec(&home)
                .env("MOCK_AGENT_NEW_SESSION_REQUIRE_AUTH", "1")
                .output()
                .unwrap();
            let stderr = String::from_utf8(output.stderr).unwrap();
            assert!(!output.status.success() && output.stdout.is_empty());
            assert!(hints.iter().all(|hint| stderr.contains(hint)), "{stderr}");
        }
    }
}

struct AcpProcess {
    child: Child,
    stdin: ChildStdin,
    messages: mpsc::Receiver<io::Result<Value>>,
}

impl AcpProcess {
    fn spawn() -> Result<(Self, TempDir), Box<dyn std::error::Error>> {
        Self::spawn_config(false)
    }

    fn spawn_config(multi_session: bool) -> Result<(Self, TempDir), Box<dyn std::error::Error>> {
        let nori_home = TempDir::new()?;
        std::fs::write(
            nori_home.path().join("config.toml"),
            format!("[features]\nmulti_session = {multi_session}\n"),
        )?;
        let nori_bin = assert_cmd::cargo::cargo_bin("nori");
        let mut child = StdCommand::new(nori_bin)
            .env("NORI_HOME", nori_home.path())
            .env("MOCK_ACP_AGENT_BIN", mock_agent_bin()?)
            .args(["--agent", "mock-model", "exec", "--acp"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("child stdin was not piped"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("child stdout was not piped"))?;
        let (message_tx, messages) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let value =
                    line.and_then(|line| serde_json::from_str(&line).map_err(io::Error::other));
                if message_tx.send(value).is_err() {
                    break;
                }
            }
        });
        Ok((
            Self {
                child,
                stdin,
                messages,
            },
            nori_home,
        ))
    }

    fn send(&mut self, message: Value) -> Result<(), Box<dyn std::error::Error>> {
        serde_json::to_writer(&mut self.stdin, &message)?;
        self.stdin.write_all(b"\n")?;
        self.stdin.flush()?;
        Ok(())
    }

    fn receive(&self) -> Result<Value, Box<dyn std::error::Error>> {
        Ok(self.messages.recv_timeout(Duration::from_secs(10))??)
    }

    fn receive_response(
        &self,
        id: i64,
        observed: &mut Vec<Value>,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        loop {
            let message = self.receive()?;
            if message.get("id") == Some(&json!(id)) {
                return Ok(message);
            }
            observed.push(message);
        }
    }

    fn initialize_session(&mut self) -> Result<(String, Vec<Value>), Box<dyn std::error::Error>> {
        let mut observed = Vec::new();
        self.send(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": 1,
                "clientCapabilities": {},
                "clientInfo": {"name": "headless-exec-test", "version": "1"}
            }
        }))?;
        let initialized = self.receive_response(1, &mut observed)?;
        assert_eq!(initialized["jsonrpc"], "2.0");
        assert_eq!(initialized["result"]["protocolVersion"], 1);
        assert_eq!(initialized["result"]["agentInfo"]["name"], "nori");

        self.send(json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "session/new",
            "params": {
                "cwd": std::env::current_dir()?,
                "mcpServers": []
            }
        }))?;
        let session = self.receive_response(2, &mut observed)?;
        let session_id = session["result"]["sessionId"]
            .as_str()
            .ok_or_else(|| io::Error::other("session/new response omitted sessionId"))?
            .to_string();
        let config_options = session["result"]["configOptions"]
            .as_array()
            .ok_or_else(|| {
                io::Error::other("session/new response omitted effective configOptions")
            })?;
        assert!(config_options.iter().any(|option| option["id"] == "model"));
        Ok((session_id, observed))
    }
}

impl Drop for AcpProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn exec_acp_is_a_standard_facade_with_one_final_message_update()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut acp, _nori_home) = AcpProcess::spawn()?;
    let (session_id, mut observed) = acp.initialize_session()?;

    acp.send(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "session/prompt",
        "params": {
            "sessionId": session_id,
            "prompt": [{"type": "text", "text": "hello"}]
        }
    }))?;
    let prompt_response = acp.receive_response(3, &mut observed)?;

    assert_eq!(prompt_response["result"]["stopReason"], "end_turn");
    assert!(observed.iter().all(|message| message["jsonrpc"] == "2.0"));
    let updates = observed
        .iter()
        .filter(|message| message["method"] == "session/update")
        .collect::<Vec<_>>();
    assert_eq!(updates.len(), 1);
    assert_eq!(
        updates[0]["params"]["update"],
        json!({
            "sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": "Test message 1Test message 2"}
        })
    );
    Ok(())
}

#[test]
fn exec_acp_forwards_permission_requests_and_correlated_responses()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut acp, _nori_home) = AcpProcess::spawn()?;
    let (session_id, _) = acp.initialize_session()?;
    acp.send(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "session/prompt",
        "params": {
            "sessionId": session_id,
            "prompt": [{"type": "text", "text": "mock:request-permission"}]
        }
    }))?;

    let mut observed = Vec::new();
    let permission = loop {
        let message = acp.receive()?;
        if message["method"] == "session/request_permission" {
            break message;
        }
        observed.push(message);
    };
    let permission_id = permission["id"].clone();
    assert!(
        permission["params"]["options"]
            .as_array()
            .is_some_and(|options| options.iter().any(|option| option["optionId"] == "allow"))
    );
    acp.send(json!({
        "jsonrpc": "2.0",
        "id": permission_id,
        "result": {"outcome": {"outcome": "selected", "optionId": "allow"}}
    }))?;

    let prompt_response = acp.receive_response(3, &mut observed)?;
    assert_eq!(prompt_response["result"]["stopReason"], "end_turn");
    let final_text = observed
        .iter()
        .find(|message| message["method"] == "session/update")
        .and_then(|message| message["params"]["update"]["content"]["text"].as_str())
        .ok_or_else(|| io::Error::other("prompt omitted final agent message"))?;
    assert!(final_text.contains("Permission granted with option: allow"));
    Ok(())
}

#[test]
fn exec_managed_stream_tags_every_event_and_routes_two_sessions()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut process, _home) = AcpProcess::spawn_config(true)?;
    let (first, mut observed) = process.initialize_session()?;
    process.send(json!({"jsonrpc":"2.0","id":10,"method":"session/new",
        "params":{"cwd":std::env::current_dir()?,"mcpServers":[]}}))?;
    let second = process.receive_response(10, &mut observed)?["result"]["sessionId"]
        .as_str()
        .ok_or("second session was not created")?
        .to_string();
    assert_ne!(first, second);
    process.send(
        json!({"jsonrpc":"2.0","id":11,"method":"_nori/session/subscribe",
        "params":{"session_id":"*","cursors":{}}}),
    )?;
    assert!(
        process
            .receive_response(11, &mut observed)?
            .get("error")
            .is_none()
    );
    for (index, id) in [&first, &second].into_iter().enumerate() {
        process.send(json!({"jsonrpc":"2.0","id":20 + index,"method":"_nori/session/prompt",
            "params":{"session_id":id,"prompt":[{"type":"text","text":format!("route marker {index}")}]}}))?;
        assert!(
            process
                .receive_response(20 + i64::try_from(index)?, &mut observed)?
                .get("error")
                .is_none()
        );
    }
    let mut cursors = std::collections::HashMap::<String, i64>::new();
    let mut completed = std::collections::HashSet::new();
    let mut routed = std::collections::HashSet::new();
    loop {
        if observed.is_empty() {
            observed.push(process.receive()?);
        }
        for message in observed.drain(..) {
            if message["method"] != "_nori/session/event" {
                continue;
            }
            let envelope = &message["params"];
            let id = envelope["conversation_id"]
                .as_str()
                .ok_or("event lacks session")?;
            assert!([first.as_str(), second.as_str()].contains(&id));
            assert_eq!(envelope["agent"], "mock-model");
            for (index, expected_id) in [&first, &second].into_iter().enumerate() {
                if envelope["event"]
                    .to_string()
                    .contains(&format!("route marker {index}"))
                {
                    assert_eq!(id, expected_id);
                    routed.insert(id.to_string());
                }
            }
            let next = cursors.entry(id.to_string()).or_default();
            *next += 1;
            assert_eq!(envelope["seq"], *next, "missing or duplicate event");
            if envelope["event"]["event"]["message_type"] == "response"
                && envelope.to_string().contains("end_turn")
            {
                completed.insert(id.to_string());
            }
        }
        if completed.len() == 2 {
            break;
        }
    }
    assert_eq!(cursors.len(), 2);
    assert_eq!(routed.len(), 2);
    Ok(())
}

#[test]
fn exec_managed_mode_preserves_standard_acp_prompt_behavior()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut process, _home) = AcpProcess::spawn_config(true)?;
    let (id, mut observed) = process.initialize_session()?;
    process.send(json!({"jsonrpc":"2.0","id":3,"method":"session/prompt",
        "params":{"sessionId":id,"prompt":[{"type":"text","text":"hello"}]}}))?;
    assert_eq!(
        process.receive_response(3, &mut observed)?["result"]["stopReason"],
        "end_turn"
    );
    let updates: Vec<_> = observed
        .iter()
        .filter(|event| event["method"] == "session/update")
        .collect();
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0]["params"]["sessionId"], id);
    assert_eq!(
        updates[0]["params"]["update"]["content"]["text"],
        "Test message 1Test message 2"
    );
    Ok(())
}

#[test]
fn exec_managed_preserves_standard_permission_answers() -> Result<(), Box<dyn std::error::Error>> {
    let (mut acp, _nori_home) = AcpProcess::spawn_config(true)?;
    let (session_id, _) = acp.initialize_session()?;
    acp.send(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "session/prompt",
        "params": {
            "sessionId": session_id,
            "prompt": [{"type": "text", "text": "mock:request-permission"}]
        }
    }))?;

    let mut observed = Vec::new();
    let permission = loop {
        let message = acp.receive()?;
        if message["method"] == "session/request_permission" {
            break message;
        }
        observed.push(message);
    };
    let permission_id = permission["id"].clone();
    assert!(
        permission["params"]["options"]
            .as_array()
            .is_some_and(|options| options.iter().any(|option| option["optionId"] == "allow"))
    );
    acp.send(json!({
        "jsonrpc": "2.0",
        "id": permission_id,
        "result": {"outcome": {"outcome": "selected", "optionId": "allow"}}
    }))?;

    let prompt_response = acp.receive_response(3, &mut observed)?;
    assert_eq!(prompt_response["result"]["stopReason"], "end_turn");
    let final_text = observed
        .iter()
        .find(|message| message["method"] == "session/update")
        .and_then(|message| message["params"]["update"]["content"]["text"].as_str())
        .ok_or_else(|| io::Error::other("prompt omitted final agent message"))?;
    assert!(final_text.contains("Permission granted with option: allow"));
    Ok(())
}

#[test]
fn exec_standard_prompt_after_extension_turn_excludes_old_output()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut process, _home) = AcpProcess::spawn_config(true)?;
    let (id, mut observed) = process.initialize_session()?;
    process.send(json!({"jsonrpc":"2.0","id":10,"method":"_nori/session/subscribe","params":{"session_id":id}}))?;
    process.receive_response(10, &mut observed)?;
    process.send(json!({"jsonrpc":"2.0","id":11,"method":"_nori/session/prompt","params":{"session_id":id,"prompt":[{"type":"text","text":"first turn"}]}}))?;
    process.receive_response(11, &mut observed)?;
    while !observed.iter().any(|message| {
        message["method"] == "_nori/session/event"
            && message["params"]["event"]["event"]["message_type"] == "response"
            && message.to_string().contains("end_turn")
    }) {
        observed.push(process.receive()?);
    }
    observed.clear();
    process.send(json!({"jsonrpc":"2.0","id":12,"method":"session/prompt","params":{"sessionId":id,"prompt":[{"type":"text","text":"second turn"}]}}))?;
    assert_eq!(
        process.receive_response(12, &mut observed)?["result"]["stopReason"],
        "end_turn"
    );
    let updates: Vec<_> = observed
        .iter()
        .filter(|message| message["method"] == "session/update")
        .collect();
    assert_eq!(updates.len(), 1);
    assert_eq!(
        updates[0]["params"]["update"]["content"]["text"],
        "Test message 1Test message 2"
    );
    Ok(())
}

#[test]
fn exec_standard_prompt_completes_when_permission_is_answered_through_managed_protocol()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut process, _home) = AcpProcess::spawn_config(true)?;
    let (session_id, mut observed) = process.initialize_session()?;
    process.send(
        json!({"jsonrpc":"2.0","id":10,"method":"_nori/session/subscribe",
        "params":{"session_id":session_id}}),
    )?;
    process.receive_response(10, &mut observed)?;
    process.send(json!({"jsonrpc":"2.0","id":11,"method":"session/prompt",
        "params":{"sessionId":session_id,"prompt":[{"type":"text","text":"mock:request-permission"}]}}))?;
    let mut forwarded = None;
    let mut original = None;
    while forwarded.is_none() || original.is_none() {
        let message = process.receive()?;
        if message["method"] == "session/request_permission" {
            forwarded = Some(message["id"].clone());
        }
        if message["method"] == "_nori/session/event"
            && message["params"]["event"]["event"]["message_type"] == "request"
        {
            original = Some(message["params"]["event"]["event"]["request_id"].clone());
        }
        observed.push(message);
    }
    process.send(
        json!({"jsonrpc":"2.0","id":12,"method":"_nori/session/respond",
        "params":{"session_id":session_id,"request_id":original.unwrap(),
            "response":{"outcome":{"outcome":"selected","optionId":"allow"}}}}),
    )?;
    assert_eq!(
        process.receive_response(12, &mut observed)?["result"]["answered"],
        true
    );
    assert_eq!(
        process.receive_response(11, &mut observed)?["result"]["stopReason"],
        "end_turn"
    );
    let update = observed
        .iter()
        .find(|message| message["method"] == "session/update")
        .ok_or("standard prompt omitted final output")?;
    assert!(
        update["params"]["update"]["content"]["text"]
            .as_str()
            .unwrap()
            .contains("Permission granted")
    );
    // A late answer to the forwarded ACP request must not poison the next turn.
    process.send(json!({"jsonrpc":"2.0","id":forwarded.unwrap(),
        "result":{"outcome":{"outcome":"selected","optionId":"allow"}}}))?;
    process.send(json!({"jsonrpc":"2.0","id":13,"method":"session/prompt",
        "params":{"sessionId":session_id,"prompt":[{"type":"text","text":"still usable"}]}}))?;
    assert_eq!(
        process.receive_response(13, &mut observed)?["result"]["stopReason"],
        "end_turn"
    );
    Ok(())
}

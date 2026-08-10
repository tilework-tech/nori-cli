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
fn exec_composes_the_argument_and_piped_stdin() -> Result<(), Box<dyn std::error::Error>> {
    let nori_home = TempDir::new()?;
    let output = nori_command(&nori_home)?
        .env("MOCK_AGENT_ECHO_PROMPT", "1")
        .args(["exec", "review this"])
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
fn print_flag_reads_the_prompt_from_stdin() -> Result<(), Box<dyn std::error::Error>> {
    let nori_home = TempDir::new()?;
    let output = nori_command(&nori_home)?
        .env("MOCK_AGENT_ECHO_PROMPT", "1")
        .arg("--print")
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

struct AcpProcess {
    child: Child,
    stdin: ChildStdin,
    messages: mpsc::Receiver<io::Result<Value>>,
}

impl AcpProcess {
    fn spawn() -> Result<(Self, TempDir), Box<dyn std::error::Error>> {
        let nori_home = TempDir::new()?;
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

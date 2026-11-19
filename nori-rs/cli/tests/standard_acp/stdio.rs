use std::fs;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::io::Lines;
use tokio::process::Child;
use tokio::process::ChildStdin;
use tokio::process::ChildStdout;

use super::FIXTURE;
use super::Host;

/// A foreground host on stdin and stdout, as a machine bridge would run it.
struct StdioHost {
    host: Host,
    child: Child,
    input: ChildStdin,
    output: Lines<BufReader<ChildStdout>>,
    cwd: PathBuf,
    home: PathBuf,
}

impl StdioHost {
    fn start(spec_env: Value, process_env: &[(&str, &str)], extra: &[&str]) -> Self {
        let host = Host::new();
        let cwd = host.0.path().join("workspace");
        let home = host.0.path().join("home");
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(&home).unwrap();
        fs::write(cwd.join("fixture-modes"), "bypassPermissions plan").unwrap();
        fs::write(
            host.0.path().join("config.toml"),
            format!(
                "[host]\nmanaged=true\nside_sessions=true\n[host.dedicated_roots]\nbinding_id='aster'\ncwd='{}'\nhome='{}'\n",
                cwd.display(),
                home.display()
            ),
        )
        .unwrap();
        let launch = host.0.path().join("launch.json");
        fs::write(
            &launch,
            json!({
                "projectId":"fixture-project", "runId":"run-aster", "agent":"aster", "model":"aster",
                "cwd":cwd, "home":home, "env":spec_env,
                "adapter":{"path":which::which("python3").unwrap(),"version":"fixture-1","args":[FIXTURE,"aster"]},
                "additions":{"promptEcho":false,"statusEvents":false,"recording":false,"clientMcp":false}
            })
            .to_string(),
        )
        .unwrap();
        let mut command =
            tokio::process::Command::from(host.command(&["host", "--stdio", "--launch"]));
        let mut child = command
            .arg(&launch)
            .args(extra)
            .envs(process_env.iter().copied())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        Self {
            input: child.stdin.take().unwrap(),
            output: BufReader::new(child.stdout.take().unwrap()).lines(),
            host,
            child,
            cwd,
            home,
        }
    }

    /// Send one ACP request and return its response. Every line the host
    /// writes must be a bare JSON-RPC message.
    async fn call(&mut self, id: i64, method: &str, params: Value) -> Value {
        let line = format!(
            "{}\n",
            json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
        );
        self.input.write_all(line.as_bytes()).await.unwrap();
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let line = self.output.next_line().await.unwrap().unwrap();
                let message: Value = serde_json::from_str(&line).unwrap();
                assert_eq!(message["jsonrpc"], "2.0", "{message}");
                assert!(message.get("bindingId").is_none(), "{message}");
                if message["id"] == id && message.get("method").is_none() {
                    return message;
                }
            }
        })
        .await
        .unwrap()
    }

    async fn open_parent(&mut self) {
        let initialized = self
            .call(1, "initialize", json!({"protocolVersion":1}))
            .await;
        assert_eq!(initialized["result"]["agentInfo"]["name"], "aster");
        let opened = self
            .call(2, "session/new", json!({"cwd":self.cwd,"mcpServers":[]}))
            .await;
        assert_eq!(opened["result"]["sessionId"], "0");
    }

    async fn prompt(&mut self, id: i64, sid: &str, text: &str) -> Value {
        self.call(
            id,
            "session/prompt",
            json!({"sessionId":sid,"prompt":[{"type":"text","text":text}]}),
        )
        .await
    }

    /// Close stdin and wait for the host to leave.
    async fn finish(mut self) -> (std::process::ExitStatus, Host) {
        drop(self.input);
        let status = tokio::time::timeout(Duration::from_secs(35), self.child.wait())
            .await
            .unwrap()
            .unwrap();
        (status, self.host)
    }
}

#[tokio::test]
async fn stdio_host_serves_the_pinned_adapter_and_a_side_session_as_plain_acp_until_stdin_closes() {
    let mut host = StdioHost::start(json!({"FIXTURE_MODEL":"aster"}), &[], &[]);
    host.open_parent().await;
    let finished = host.prompt(3, "0", "finish").await;
    assert_eq!(finished["result"]["stopReason"], "end_turn");

    let mark = json!({"bindingId":"aster","generation":1,"sessionId":"0"});
    let side = host
        .call(
            4,
            "session/new",
            json!({"cwd":host.cwd,"mcpServers":[],"_meta":{"nori.dev/sideSession":mark}}),
        )
        .await;
    assert_eq!(
        side["result"]["_meta"]["nori.dev/sideSession"], mark,
        "{side}"
    );
    let side = side["result"]["sessionId"].as_str().unwrap().to_owned();
    assert_ne!(side, "0");
    let settings = host.prompt(5, &side, "inspect-settings").await;
    assert_eq!(
        settings["result"]["_meta"],
        json!({"mode":"plan","model":"aster"})
    );
    let parent = host.prompt(6, "0", "finish").await;
    assert_eq!(parent["result"]["stopReason"], "end_turn");

    let (status, files) = host.finish().await;
    assert!(status.success(), "{status}");
    assert!(!files.0.path().join("host/state.json").exists());
}

#[tokio::test]
async fn stdio_host_exits_and_ends_its_adapter_when_terminated_with_stdin_still_open() {
    let mut host = StdioHost::start(json!({"FIXTURE_MODEL":"aster"}), &[], &[]);
    fs::write(host.cwd.join("specimen.txt"), "cedar").unwrap();
    fs::write(host.home.join("fixture-credential"), "synthetic-access").unwrap();
    host.open_parent().await;
    // A machine bridge stops its child with a signal and keeps the pipes.
    let adapter = fs::File::create(host.cwd.join("hold-lease-adapter")).unwrap();
    fs::File::create(host.cwd.join("hold-lease-adapter-client")).unwrap();
    let held = host.prompt(3, "0", "hold-lease-adapter").await;
    assert_eq!(held["result"]["stopReason"], "end_turn", "{held}");
    assert!(matches!(
        adapter.try_lock(),
        Err(fs::TryLockError::WouldBlock)
    ));

    let pid = i32::try_from(host.child.id().unwrap()).unwrap();
    // SAFETY: signals the child this test started and still owns.
    assert_eq!(unsafe { libc::kill(pid, libc::SIGTERM) }, 0);
    let status = tokio::time::timeout(Duration::from_secs(20), host.child.wait())
        .await
        .expect("the host is still running after the signal")
        .unwrap();
    assert!(!status.success());
    adapter.try_lock().expect("the adapter outlived its host");
    drop(host.input);
}

#[tokio::test]
async fn stdio_host_can_give_the_adapter_its_own_environment_but_not_its_home() {
    let mut host = StdioHost::start(
        json!({}),
        &[
            ("FIXTURE_MODEL", "aster"),
            ("HOME", "/nonexistent-caller-home"),
        ],
        &["--inherit-env"],
    );
    fs::write(host.cwd.join("specimen.txt"), "cedar").unwrap();
    fs::write(host.home.join("fixture-credential"), "synthetic-access").unwrap();
    host.open_parent().await;
    let report = host.prompt(3, "0", "inspect-workspace").await;
    let observed = &report["result"]["_meta"];
    assert_eq!(observed["model"], "aster", "{report}");
    assert_eq!(observed["home"], host.home.to_str().unwrap());
    assert_eq!(observed["cwd"], host.cwd.to_str().unwrap());
    assert!(host.finish().await.0.success());
}

#[tokio::test]
async fn stdio_host_exits_with_an_error_and_a_clean_stdout_when_the_launch_is_refused() {
    let mut host = StdioHost::start(json!({"HOME":"/reserved"}), &[], &[]);
    let status = tokio::time::timeout(Duration::from_secs(35), host.child.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(!status.success());
    assert_eq!(host.output.next_line().await.unwrap(), None);
    let mut reason = String::new();
    host.child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut reason)
        .await
        .unwrap();
    assert!(reason.contains("environment key"), "{reason}");
}

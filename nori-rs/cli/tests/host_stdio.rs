//! `nori host --stdio` without a launch file, and its alias
//! `nori exec --acp`: a headless local host whose one client speaks ACP v1 on
//! stdin and stdout for as long as stdin stays open.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::io::Lines;
use tokio::process::Child;
use tokio::process::ChildStdin;
use tokio::process::ChildStdout;

fn mock_agent_binary() -> String {
    std::env::var("MOCK_ACP_AGENT_BIN").unwrap_or_else(|_| {
        Path::new(env!("CARGO_BIN_EXE_nori"))
            .with_file_name("mock_acp_agent")
            .display()
            .to_string()
    })
}

/// A Nori home whose default agent is the mock.
fn home() -> TempDir {
    let home = TempDir::new().unwrap();
    std::fs::write(
        home.path().join("config.toml"),
        "agent = \"mock-model\"\napproval_policy = \"on-request\"\n",
    )
    .unwrap();
    home
}

fn canonical(path: &Path) -> String {
    path.canonicalize().unwrap().display().to_string()
}

fn text(text: &str) -> Value {
    json!([{"type": "text", "text": text}])
}

fn nori(home: &Path) -> std::process::Command {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_nori"));
    command
        .env("NORI_HOME", home)
        .env("MOCK_ACP_AGENT_BIN", mock_agent_binary());
    command
}

/// One headless host process and its client end of the pipes.
struct StdioHost {
    child: Child,
    input: Option<ChildStdin>,
    output: Lines<BufReader<ChildStdout>>,
    next_id: i64,
    /// Frames read while waiting for something else, not yet taken.
    inbox: Vec<Value>,
    /// Every line read from stdout, in order.
    lines: Vec<String>,
}

impl StdioHost {
    fn start(home: &Path, args: &[&str], env: &[(&str, &str)]) -> Self {
        let mut command = tokio::process::Command::from(nori(home));
        let mut child = command
            .args(args)
            .envs(env.iter().copied())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        Self {
            input: child.stdin.take(),
            output: BufReader::new(child.stdout.take().unwrap()).lines(),
            child,
            next_id: 1,
            inbox: Vec::new(),
            lines: Vec::new(),
        }
    }

    fn host(home: &Path, env: &[(&str, &str)]) -> Self {
        Self::start(home, &["host", "--stdio"], env)
    }

    async fn send(&mut self, message: Value) {
        let input = self.input.as_mut().expect("stdin is open");
        input
            .write_all(format!("{message}\n").as_bytes())
            .await
            .unwrap();
        input.flush().await.unwrap();
    }

    async fn read(&mut self) -> Value {
        let line = self
            .output
            .next_line()
            .await
            .unwrap()
            .expect("the host closed stdout");
        self.lines.push(line.clone());
        serde_json::from_str(&line).unwrap_or_else(|_| panic!("stdout is not JSON: {line}"))
    }

    /// The first frame matching `wanted`, from the inbox or the pipe.
    async fn until(&mut self, wanted: impl Fn(&Value) -> bool) -> Value {
        if let Some(index) = self.inbox.iter().position(&wanted) {
            return self.inbox.remove(index);
        }
        tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                let frame = self.read().await;
                if wanted(&frame) {
                    return frame;
                }
                self.inbox.push(frame);
            }
        })
        .await
        .unwrap_or_else(|_| panic!("the expected frame never arrived: {:#?}", self.inbox))
    }

    async fn call(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await;
        let response = self
            .until(|frame| frame.get("method").is_none() && frame["id"] == id)
            .await;
        assert!(
            response.get("error").is_none(),
            "{method} failed: {response}"
        );
        response["result"].clone()
    }

    async fn initialize(&mut self, params: Value) -> Value {
        self.call("initialize", params).await
    }

    async fn new_session(&mut self, folder: &Path) -> String {
        let result = self
            .call(
                "session/new",
                json!({"cwd": canonical(folder), "mcpServers": []}),
            )
            .await;
        result["sessionId"].as_str().unwrap().to_owned()
    }

    /// Run one turn and return its `stopReason`, checking that the turn's
    /// updates arrived before its result.
    async fn turn(&mut self, session: &str, prompt: &str) -> Value {
        let id = format!("turn-{}", self.next_id);
        self.next_id += 1;
        self.inbox.clear();
        self.send(json!({"jsonrpc":"2.0","id":id,"method":"session/prompt","params":{"sessionId":session,"prompt":text(prompt)}}))
            .await;
        let ended = self.until(|frame| frame["id"] == id.as_str()).await;
        assert!(
            self.inbox
                .iter()
                .any(|frame| frame["method"] == "session/update"
                    && frame["params"]["sessionId"] == session),
            "the turn's updates precede its result: {:#?}",
            self.inbox
        );
        ended["result"]["stopReason"].clone()
    }

    /// Close stdin and wait for the process to exit.
    async fn close(mut self) -> (std::process::ExitStatus, Vec<String>) {
        drop(self.input.take());
        let status = tokio::time::timeout(Duration::from_secs(35), self.child.wait())
            .await
            .expect("the host exits once stdin closes")
            .unwrap();
        while let Ok(Some(line)) = self.output.next_line().await {
            self.lines.push(line);
        }
        (status, self.lines)
    }
}

fn wait_for_file(path: &Path) {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !path.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "{} was never written",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn one_stdio_stream_runs_turns_in_several_sessions() {
    let home = home();
    let first = TempDir::new().unwrap();
    let second = TempDir::new().unwrap();
    let mut host = StdioHost::host(home.path(), &[("MOCK_AGENT_WRITE_FILE", "marker.txt")]);

    let initialized = host
        .initialize(json!({"protocolVersion": 1, "clientCapabilities": {}}))
        .await;
    let one = host.new_session(first.path()).await;
    let two = host.new_session(second.path()).await;
    let first_stop = host.turn(&one, "hello one").await;
    let second_stop = host.turn(&two, "hello two").await;

    assert_eq!(initialized["protocolVersion"], 1);
    assert_eq!(initialized["agentInfo"]["name"], "nori-host");
    assert!(
        initialized["agentCapabilities"]["sessionCapabilities"]["list"].is_object(),
        "{initialized}"
    );
    assert_ne!(one, two);
    assert_eq!(first_stop, "end_turn");
    assert_eq!(second_stop, "end_turn");
    wait_for_file(&first.path().join("marker.txt"));
    wait_for_file(&second.path().join("marker.txt"));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_stdio_client_answers_permission_requests_even_when_it_asks_to_observe() {
    let home = home();
    let folder = TempDir::new().unwrap();
    let mut host = StdioHost::host(home.path(), &[("MOCK_AGENT_REQUEST_PERMISSION", "1")]);
    host.initialize(json!({"protocolVersion": 1, "observe": true}))
        .await;
    let session = host.new_session(folder.path()).await;

    host.send(json!({"jsonrpc":"2.0","id":"turn","method":"session/prompt","params":{"sessionId":session,"prompt":text("run the command")}}))
        .await;
    let request = host
        .until(|frame| frame["method"] == "session/request_permission" && frame.get("id").is_some())
        .await;
    let allow = request["params"]["options"]
        .as_array()
        .unwrap()
        .iter()
        .find(|option| option["kind"] == "allow_once")
        .unwrap_or_else(|| panic!("an allow option is offered: {request}"))["optionId"]
        .clone();
    host.send(json!({"jsonrpc":"2.0","id":request["id"],"result":{"outcome":{"outcome":"selected","optionId":allow}}}))
        .await;
    let ended = host.until(|frame| frame["id"] == "turn").await;

    assert_eq!(request["params"]["sessionId"], session.as_str());
    assert_eq!(ended["result"]["stopReason"], "end_turn", "{ended}");
    assert!(
        host.inbox
            .iter()
            .any(|frame| frame["method"] == "session/update"
                && frame["params"]["update"]
                    .to_string()
                    .contains("Permission granted")),
        "the answer reached the agent: {:#?}",
        host.inbox
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn closing_stdin_ends_the_host_cleanly_with_only_acp_on_stdout() {
    let home = home();
    let folder = TempDir::new().unwrap();
    let mut host = StdioHost::host(home.path(), &[]);
    host.initialize(json!({"protocolVersion": 1})).await;
    let session = host.new_session(folder.path()).await;
    host.turn(&session, "hello").await;
    let input = host.input.as_mut().unwrap();
    input.write_all(b"not json\n").await.unwrap();
    input.write_all(b"\xff\xfe{}\n").await.unwrap();
    let refused = host.until(|frame| frame.get("error").is_some()).await;
    let not_utf8 = host.until(|frame| frame.get("error").is_some()).await;
    let still_serving = host.call("session/list", json!({})).await;

    let (status, lines) = host.close().await;

    assert!(status.success(), "{status}");
    assert_eq!(refused["error"]["code"], -32700, "{refused}");
    assert_eq!(not_utf8["error"]["code"], -32700, "{not_utf8}");
    assert!(still_serving["sessions"].is_array(), "{still_serving}");
    assert!(
        !home.path().join("host/host.json").exists()
            && !home.path().join("host/state.json").exists(),
        "a stdio host publishes no discovery"
    );
    for line in &lines {
        let frame: Value =
            serde_json::from_str(line).unwrap_or_else(|_| panic!("not an ACP line: {line}"));
        assert_eq!(frame["jsonrpc"], "2.0", "{line}");
    }
}

/// Stops the home's background host however the test ends.
struct BackgroundHost<'a>(&'a Path);

impl Drop for BackgroundHost<'_> {
    fn drop(&mut self) {
        let _ = nori(self.0)
            .args(["host", "stop"])
            .stdin(Stdio::null())
            .output();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stdio_host_leaves_the_homes_background_host_alone() {
    let home = home();
    let _background = BackgroundHost(home.path());
    let started = nori(home.path())
        .args(["host", "--port", "0"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(started.status.success(), "{started:?}");
    let discovery = std::fs::read_to_string(home.path().join("host/host.json")).unwrap();
    let folder = TempDir::new().unwrap();

    let mut host = StdioHost::host(home.path(), &[]);
    host.initialize(json!({"protocolVersion": 1})).await;
    let session = host.new_session(folder.path()).await;
    host.turn(&session, "hello").await;
    let (status, _) = host.close().await;
    let after = std::fs::read_to_string(home.path().join("host/host.json")).unwrap();
    let listed = nori(home.path())
        .args(["host", "ls"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stopped = nori(home.path())
        .args(["host", "stop"])
        .stdin(Stdio::null())
        .output()
        .unwrap();

    assert!(status.success(), "{status}");
    assert_eq!(
        after, discovery,
        "the background host's discovery is untouched"
    );
    assert!(listed.status.success(), "{listed:?}");
    assert!(stopped.status.success(), "{stopped:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn exec_acp_is_the_headless_host() {
    let home = home();
    let first = TempDir::new().unwrap();
    let second = TempDir::new().unwrap();
    let mut host = StdioHost::start(home.path(), &["exec", "--acp"], &[]);

    let initialized = host.initialize(json!({"protocolVersion": 1})).await;
    let one = host.new_session(first.path()).await;
    let two = host.new_session(second.path()).await;
    let first_stop = host.turn(&one, "hello one").await;
    let again = host.turn(&one, "hello again").await;
    let second_stop = host.turn(&two, "hello two").await;
    let (status, _) = host.close().await;

    assert_eq!(initialized["agentInfo"]["name"], "nori-host");
    assert_eq!(
        [first_stop, again, second_stop],
        [json!("end_turn"), json!("end_turn"), json!("end_turn")]
    );
    assert!(status.success(), "{status}");
}

#[tokio::test(flavor = "multi_thread")]
async fn list_rows_say_whether_a_session_is_live() {
    let home = home();
    let folder = TempDir::new().unwrap();
    let mut earlier = StdioHost::host(home.path(), &[]);
    earlier.initialize(json!({"protocolVersion": 1})).await;
    let stored = earlier.new_session(folder.path()).await;
    earlier.turn(&stored, "remember me").await;
    let (status, _) = earlier.close().await;
    assert!(status.success(), "{status}");

    let mut host = StdioHost::host(home.path(), &[]);
    host.initialize(json!({"protocolVersion": 1})).await;
    let running = host.new_session(folder.path()).await;
    let listing = host.call("session/list", json!({})).await;
    let live = |id: &str| {
        listing["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["sessionId"] == id)
            .unwrap_or_else(|| panic!("{id} is listed: {listing}"))["_meta"]["nori/session"]
            .clone()
    };

    assert_eq!(live(&running), json!({"live": true}));
    assert_eq!(live(&stored), json!({"live": false}));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stop_request_is_answered_before_the_host_exits() {
    let home = home();
    let folder = TempDir::new().unwrap();
    let mut host = StdioHost::host(home.path(), &[]);
    host.initialize(json!({"protocolVersion": 1})).await;
    host.new_session(folder.path()).await;

    let stopped = host.call("_nori/host/stop", json!({})).await;
    let status = tokio::time::timeout(Duration::from_secs(35), host.child.wait())
        .await
        .expect("the host exits after a stop request")
        .unwrap();

    assert!(stopped.is_object(), "{stopped}");
    assert!(status.success(), "{status}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_terminate_signal_stops_the_host_with_an_error() {
    let home = home();
    let folder = TempDir::new().unwrap();
    let mut host = StdioHost::host(home.path(), &[]);
    host.initialize(json!({"protocolVersion": 1})).await;
    host.new_session(folder.path()).await;

    let pid = host.child.id().unwrap().to_string();
    let killed = std::process::Command::new("kill")
        .args(["-TERM", &pid])
        .status()
        .unwrap();
    let status = tokio::time::timeout(Duration::from_secs(35), host.child.wait())
        .await
        .expect("the host exits on SIGTERM")
        .unwrap();

    assert!(killed.success());
    assert!(!status.success(), "{status}");
}

#[test]
fn a_managed_home_serves_stdio_only_with_a_launch_file() {
    let home = TempDir::new().unwrap();
    std::fs::write(home.path().join("config.toml"), "[host]\nmanaged = true\n").unwrap();

    let output = nori(home.path())
        .args(["host", "--stdio"])
        .stdin(Stdio::null())
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("--launch"),
        "{output:?}"
    );
}

//! An attached controller cancels the external turn it saw, even with input in flight.
#![cfg(unix)]

use std::fs;
use std::io::ErrorKind;
use std::net::TcpListener;
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use serde_json::Value;
use serde_json::json;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TuiSession;
use tungstenite::Message;
use tungstenite::WebSocket;
use tungstenite::client::IntoClientRequest;

const CANCEL_OBSERVED: &str = "_nori/session/cancel_observed";

struct Host {
    home: tempfile::TempDir,
    binary: PathBuf,
}

impl Host {
    fn start() -> Self {
        let home = tempfile::tempdir().unwrap();
        let cwd = home.path().canonicalize().unwrap();
        let script = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../cli/tests/standard_acp/fixture.py"
        );
        fs::write(
            home.path().join("config.toml"),
            format!(
                r#"
agent = "external-turn"
[projects.{cwd:?}]
trust_level = "trusted"
[[agents]]
name = "External Turn"
slug = "external-turn"
[agents.distribution.local]
command = "python3"
args = ["{script}", "external-turn"]
"#
            ),
        )
        .unwrap();
        let binary = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("nori");
        let host = Self { home, binary };
        let output = host.command(&["--agent", "external-turn", "host", "--port", "0"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        host
    }

    fn command(&self, args: &[&str]) -> std::process::Output {
        Command::new(&self.binary)
            .args(args)
            .env("NORI_HOME", self.home.path())
            .env("CODEX_HOME", self.home.path())
            .env("NORI_NO_ANALYTICS", "1")
            .current_dir(self.home.path())
            .output()
            .unwrap()
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.command(&["host", "stop"]);
    }
}

/// Forward the attached PTY's real WebSocket traffic. The test can withhold a
/// cancel while all other messages, including live host observations, proceed.
struct Attach {
    tui: TuiSession,
    from_tui: WebSocket<TcpStream>,
    to_host: WebSocket<TcpStream>,
    host_frames: Vec<Value>,
    session_id: String,
    hold_cancel: bool,
    held: Option<Value>,
    _host: Host,
}

fn read(socket: &mut WebSocket<TcpStream>) -> Option<Message> {
    match socket.read() {
        Ok(message) => Some(message),
        Err(tungstenite::Error::Io(e))
            if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
        {
            None
        }
        Err(e) => panic!("reading relay socket: {e}"),
    }
}

impl Attach {
    fn open() -> Self {
        let host = Host::start();
        let state: Value =
            serde_json::from_slice(&fs::read(host.home.path().join("host/state.json")).unwrap())
                .unwrap();
        let mut request = state["endpoint"]
            .as_str()
            .unwrap()
            .into_client_request()
            .unwrap();
        let token_file = host.home.path().join("host/token");
        let token = fs::read_to_string(&token_file).unwrap();
        request.headers_mut().insert(
            "Authorization",
            format!("Bearer {}", token.trim()).parse().unwrap(),
        );
        let upstream = TcpStream::connect(request.uri().authority().unwrap().as_str()).unwrap();
        let (to_host, _) = tungstenite::client(request, upstream).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("ws://{}/sessions", listener.local_addr().unwrap());
        let mut config = SessionConfig::new()
            .with_command_args([
                "--agent",
                "external-turn",
                "attach",
                "--host",
                &endpoint,
                "--token-file",
                token_file.to_str().unwrap(),
            ])
            .with_agent_env("NORI_HOME", host.home.path().to_str().unwrap())
            .with_agent_env("CODEX_HOME", host.home.path().to_str().unwrap())
            .with_agent_env("NORI_NO_ANALYTICS", "1");
        config.cwd = Some(host.home.path().to_owned());
        let mut tui = TuiSession::spawn_with_config(36, 110, config).unwrap();
        let deadline = Instant::now() + TIMEOUT;
        let stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    tui.poll().unwrap();
                    assert!(
                        Instant::now() < deadline,
                        "attach did not connect: {}",
                        tui.screen_contents()
                    );
                    thread::sleep(Duration::from_millis(5));
                }
                Err(e) => panic!("accepting attached client: {e}"),
            }
        };
        // macOS hands out accepted sockets with the listener's non-blocking
        // flag, which would interrupt the handshake.
        stream.set_nonblocking(false).unwrap();
        let from_tui = tungstenite::accept(stream).unwrap();
        for socket in [&from_tui, &to_host] {
            socket.get_ref().set_nonblocking(true).unwrap();
        }
        let mut client = Self {
            tui,
            from_tui,
            to_host,
            host_frames: Vec::new(),
            session_id: String::new(),
            hold_cancel: false,
            held: None,
            _host: host,
        };
        client.screen("Sessions");
        client.tui.send_key(Key::Escape).unwrap();
        client.screen("No session selected");
        client.tui.submit_input("/new").unwrap();
        client.until("selected subscription", |c| !c.session_id.is_empty());
        client.screen("ready");
        client
    }

    fn until(&mut self, label: &str, condition: impl Fn(&Self) -> bool) {
        let deadline = Instant::now() + TIMEOUT;
        while !condition(self) {
            self.tui.poll().unwrap();
            if let Some(message) = read(&mut self.from_tui) {
                let frame = message
                    .to_text()
                    .ok()
                    .and_then(|s| serde_json::from_str::<Value>(s).ok());
                if let Some(frame) = frame.as_ref()
                    && frame["method"] == "_nori/session/subscribe"
                    && let Some(id) = frame["params"]["session_id"].as_str().filter(|s| *s != "*")
                {
                    self.session_id = id.to_owned();
                }
                if self.hold_cancel
                    && frame.as_ref().is_some_and(|f| {
                        f["method"] == CANCEL_OBSERVED || f["method"] == "session/cancel"
                    })
                {
                    assert!(self.held.is_none(), "second cancel while holding the first");
                    self.held = frame;
                } else {
                    self.to_host.send(message).unwrap();
                }
            }
            if let Some(message) = read(&mut self.to_host) {
                if let Ok(text) = message.to_text()
                    && let Ok(frame) = serde_json::from_str(text)
                {
                    self.host_frames.push(frame);
                }
                self.from_tui.send(message).unwrap();
            }
            assert!(
                Instant::now() < deadline,
                "waiting for {label}: {}",
                self.tui.screen_contents()
            );
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn screen(&mut self, text: &str) {
        self.until(text, |c| c.tui.screen_contents().contains(text));
    }

    // The existing adapter changes its own activity on a speed setting and
    // reports its cumulative count of actual upstream session/cancel packets.
    fn activity(&mut self, working: bool, cancel_count: usize) {
        self.host_frames.clear();
        let request = json!({
            "jsonrpc": "2.0",
            "id": "fixture-speed",
            "method": "session/set_config_option",
            "params": {
                "sessionId": self.session_id,
                "configId": "speed",
                "value": if working { "fast" } else { "slow" },
            }
        });
        self.to_host
            .send(Message::text(request.to_string()))
            .unwrap();
        let title = format!(
            "{} with {cancel_count} cancels",
            if working { "working" } else { "idle" }
        );
        self.adapter_title(&title);
    }

    fn adapter_title(&mut self, title: &str) {
        self.until(title, |c| {
            c.host_frames.iter().any(|frame| {
                frame
                    .pointer("/params/event/event/update/title")
                    .and_then(Value::as_str)
                    == Some(title)
            })
        });
    }

    fn observed_turn(&mut self) -> String {
        let find = |c: &Self| {
            c.host_frames.iter().find_map(|frame| {
                let event = &frame["params"]["event"]["event"];
                (event["event_type"] == "external_turn_changed"
                    && event["event"]["state"] == "working")
                    .then(|| event["event"]["id"].as_str().unwrap().to_owned())
            })
        };
        self.until("external working observation", |c| find(c).is_some());
        find(self).unwrap()
    }
}

impl Drop for Attach {
    fn drop(&mut self) {
        self.tui.terminate_and_wait(Duration::from_secs(2));
    }
}

#[test]
fn ctrl_c_cancels_once_and_preserves_the_attached_draft() {
    let mut client = Attach::open();
    client.activity(true, 0);
    client.screen("Agent working");
    let draft = "Remember the seven bridges";
    client.tui.type_input(draft).unwrap();
    client.tui.send_key(Key::Ctrl('c')).unwrap();
    client.screen("Cancel requested");
    client.adapter_title("Received cancel 1");
    assert!(
        client.tui.screen_contents().contains(draft),
        "Ctrl+C erased the draft"
    );
    client.tui.send_key(Key::Ctrl('c')).unwrap();
    client.tui.send_str("!").unwrap();
    let edited = format!("{draft}!");
    client.screen(&edited);
    client.activity(false, 1);
    client.screen("ready");
    assert!(client.tui.screen_contents().contains(&edited));
}

#[test]
fn delayed_cancel_for_amber_does_not_interrupt_blue() {
    let mut client = Attach::open();
    client.activity(true, 0);
    let amber = client.observed_turn();
    client.screen("Agent working");
    client.hold_cancel = true;
    client.tui.submit_input("/cancel").unwrap();
    client.until("Amber's outgoing cancel", |c| c.held.is_some());

    client.activity(false, 0);
    client.screen("ready");
    client.activity(true, 0);
    let blue = client.observed_turn();
    assert_ne!(amber, blue);
    client.screen("Agent working");

    let late = client.held.take().unwrap();
    client.hold_cancel = false;
    client
        .to_host
        .send(Message::text(late.to_string()))
        .unwrap();
    assert_eq!(late["method"], CANCEL_OBSERVED);
    assert_eq!(late["params"]["sessionId"], client.session_id);
    assert_eq!(late["params"]["observedTurnId"], amber);
    client.until("host rejection of Amber's late cancel", |c| {
        c.host_frames.iter().any(|f| {
            f["id"] == late["id"]
                && f["error"]["message"]
                    .as_str()
                    .is_some_and(|m| m.contains("no longer active"))
        })
    });
    client.activity(true, 0);
    client.tui.send_key(Key::Ctrl('c')).unwrap();
    client.screen("Cancel requested");
    client.adapter_title("Received cancel 1");
    client.activity(false, 1);
}

#[test]
fn escape_in_attach_reaches_the_terminal_submitted_prompt_once() {
    let mut client = Attach::open();
    client.tui.submit_input("amber").unwrap();
    client.screen("Awaiting interrupt");
    client.tui.send_key(Key::Escape).unwrap();
    client.tui.send_key(Key::Escape).unwrap();
    client.adapter_title("Received cancel 1");
    client.activity(false, 1);
    client.screen("ready");
}

#[test]
fn early_escape_in_plain_nori_waits_for_its_admission() {
    let home = tempfile::tempdir().unwrap();
    let folder = home.path().canonicalize().unwrap();
    let hook = folder.join("gate.sh");
    fs::write(
        &hook,
        r#"cd "$(dirname "$0")"
touch entered
while [ ! -e release ]; do sleep 0.02; done
"#,
    )
    .unwrap();
    let trace = folder.join("fixture-traffic");
    fs::write(&trace, "").unwrap();
    let script = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../cli/tests/standard_acp/fixture.py"
    );
    fs::write(
        folder.join("config.toml"),
        format!(
            r#"agent = "external-turn"
[projects.{folder:?}]
trust_level = "trusted"
[hooks]
pre_user_prompt = [{hook:?}]
[[agents]]
name = "External Turn"
slug = "external-turn"
[agents.distribution.local]
command = "python3"
args = [{script:?}, "external-turn"]
"#
        ),
    )
    .unwrap();
    let config = SessionConfig::new()
        .with_agent("external-turn".into())
        .with_cwd(&folder)
        .with_nori_home(&folder)
        .with_arg("amber");
    let mut tui = TuiSession::spawn_with_config(36, 110, config).unwrap();
    tui.wait_for(|_| folder.join("entered").exists(), TIMEOUT)
        .unwrap();
    tui.send_key(Key::Escape).unwrap();
    tui.send_key(Key::Escape).unwrap();
    // A composer edit is a terminal-side barrier before the hook is released.
    tui.type_input("draft").unwrap();
    tui.wait_for_text("draft", TIMEOUT).unwrap();
    assert!(
        !fs::read_to_string(&trace)
            .unwrap()
            .contains("session/cancel")
    );
    fs::write(folder.join("release"), "").unwrap();
    tui.wait_for(
        |_| {
            fs::read_to_string(&trace)
                .unwrap()
                .contains("session/cancel")
        },
        TIMEOUT,
    )
    .unwrap();
    let traffic = fs::read_to_string(&trace).unwrap();
    let calls: Vec<_> = traffic
        .lines()
        .filter(|line| line.starts_with("session/prompt ") || line.starts_with("session/cancel "))
        .collect();
    assert_eq!(calls.len(), 2, "{traffic}");
    assert_eq!(calls[0].replace("prompt", "cancel"), calls[1]);
    tui.wait_for_text("draft", TIMEOUT).unwrap();
}

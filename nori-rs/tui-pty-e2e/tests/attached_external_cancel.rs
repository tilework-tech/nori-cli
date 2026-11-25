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
    /// Every request the terminal sent the host, in order.
    tui_requests: Vec<Value>,
    session_id: String,
    hold_cancel: bool,
    held: Option<Value>,
    /// The host endpoint and token, for a second terminal's connection.
    endpoint: tungstenite::handshake::client::Request,
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
        let endpoint_request = request.clone();
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
            tui_requests: Vec::new(),
            session_id: String::new(),
            hold_cancel: false,
            held: None,
            endpoint: endpoint_request,
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
                if let Some(frame) = frame.as_ref().filter(|f| f["method"].is_string()) {
                    self.tui_requests.push(frame.clone());
                }
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

    /// Another terminal joins the host and takes control, so this one
    /// watches.
    fn lose_control(&mut self) -> WebSocket<TcpStream> {
        let upstream =
            TcpStream::connect(self.endpoint.uri().authority().unwrap().as_str()).unwrap();
        let (mut other, _) = tungstenite::client(self.endpoint.clone(), upstream).unwrap();
        for (id, method, params) in [
            (1, "initialize", json!({})),
            (2, "_nori/control/take", json!({})),
        ] {
            let request = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
            other.send(Message::text(request.to_string())).unwrap();
        }
        self.screen("Watching");
        other
    }

    /// The interrupts this terminal has sent the host.
    fn interrupts_sent(&self) -> usize {
        self.tui_requests
            .iter()
            .filter(|f| f["method"] == CANCEL_OBSERVED || f["method"] == "session/cancel")
            .count()
    }

    /// Relay for `duration`, so anything the terminal would send arrives.
    fn relay_for(&mut self, duration: Duration) {
        let until = Instant::now() + duration;
        self.until("the relay to settle", |_| Instant::now() >= until);
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

/// The status row a running turn shows, whoever started it.
const TURN_RUNNING: &str = "esc to interrupt";

#[test]
fn ctrl_c_cancels_once_and_preserves_the_attached_draft() {
    let mut client = Attach::open();
    client.activity(true, 0);
    client.screen(TURN_RUNNING);
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
    client.screen(TURN_RUNNING);
    client.hold_cancel = true;
    client.tui.submit_input("/cancel").unwrap();
    client.until("Amber's outgoing cancel", |c| c.held.is_some());

    client.activity(false, 0);
    client.screen("ready");
    client.activity(true, 0);
    let blue = client.observed_turn();
    assert_ne!(amber, blue);
    client.screen(TURN_RUNNING);

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
    // The adapter's reply has no final newline, so the terminal holds it
    // back until the turn ends; the host shows the prompt was admitted.
    client.until("the prompt's first reply", |c| {
        c.host_frames.iter().any(|frame| {
            frame
                .pointer("/params/event/event/update/content/text")
                .and_then(Value::as_str)
                == Some("Awaiting interrupt")
        })
    });
    client.screen(TURN_RUNNING);
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

#[test]
fn attached_close_can_be_refused_and_retried() {
    let mut client = Attach::open();
    let folder = client._host.home.path().to_owned();
    fs::write(folder.join("fixture-traffic"), "").unwrap();
    fs::write(folder.join("fixture-refuse-close"), "").unwrap();
    client.tui.submit_input("/close").unwrap();
    client.screen("sample close declined");
    client.tui.submit_input("/close").unwrap();
    client.until("session ended after retry", |c| {
        c.host_frames.iter().any(|frame| {
            frame.pointer("/params/event/event/event_type") == Some(&json!("session_ended"))
                && frame.pointer("/params/event/event/event/reason") == Some(&json!("closed"))
        })
    });
    let trace = fs::read_to_string(folder.join("fixture-traffic")).unwrap();
    assert_eq!(
        trace
            .lines()
            .filter(|line| line.starts_with("session/close "))
            .count(),
        2,
        "{trace}"
    );
    assert!(!trace.contains("session/cancel"), "{trace}");
}

#[test]
fn a_watchers_ctrl_c_leaves_the_controllers_turn_running() {
    let mut client = Attach::open();
    client.activity(true, 0);
    client.screen(TURN_RUNNING);
    let _other = client.lose_control();
    let draft = "Watching the tide come in";
    client.tui.type_input(draft).unwrap();
    let before = client.tui_requests.len();

    client.tui.send_key(Key::Ctrl('c')).unwrap();
    client.tui.send_key(Key::Escape).unwrap();
    client.screen("Alt+T");
    client.relay_for(Duration::from_millis(500));

    assert_eq!(client.interrupts_sent(), 0);
    assert_eq!(
        client.tui_requests.len(),
        before,
        "{:?}",
        &client.tui_requests[before..]
    );
    let screen = client.tui.screen_contents();
    assert!(
        screen.contains(draft),
        "the watcher's draft is kept: {screen}"
    );
    assert!(
        screen.contains(TURN_RUNNING),
        "the turn keeps running: {screen}"
    );
}

#[test]
fn ctrl_c_with_nothing_typed_leaves_the_host_once_a_cancel_is_pending() {
    let mut client = Attach::open();
    client.activity(true, 0);
    client.screen(TURN_RUNNING);
    client.tui.send_key(Key::Ctrl('c')).unwrap();
    client.screen("Cancel requested");
    client.adapter_title("Received cancel 1");

    client.tui.send_key(Key::Ctrl('c')).unwrap();

    // Leaving closes this terminal's connection; the host keeps the turn.
    assert!(
        client.tui.wait_for_process_exit(TIMEOUT),
        "{}",
        client.tui.screen_contents()
    );
    assert_eq!(client.interrupts_sent(), 1);
}

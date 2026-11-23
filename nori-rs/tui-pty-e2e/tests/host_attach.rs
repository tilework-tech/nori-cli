//! Detached hosts and independent attached terminal clients.
#![cfg(target_os = "linux")]

use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::net::Shutdown;
use std::net::SocketAddr;
use std::net::TcpListener;
use std::net::TcpStream;
use std::ops::Deref;
use std::ops::DerefMut;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;
use std::process::Stdio;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;
use std::time::Instant;

use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TuiSession;

const ANSWER: &str = "D2 host agent response";
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(35);

/// Own the host's home until all clients have been dropped and the host stopped.
struct Host {
    home: tempfile::TempDir,
    binary: PathBuf,
    steering: bool,
}

impl Host {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        let cwd = home.path().join("project");
        std::fs::create_dir(&cwd).unwrap();
        let quoted_cwd = serde_json::to_string(&cwd.to_string_lossy()).unwrap();
        std::fs::write(
            home.path().join("config.toml"),
            format!(
                "agent = \"mock-model\"\n\
                 [projects.{quoted_cwd}]\ntrust_level = \"trusted\"\n"
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
        Self {
            home,
            binary,
            steering: false,
        }
    }

    fn args(args: &[&str]) -> Vec<String> {
        ["--agent", "mock-model"]
            .into_iter()
            .chain(args.iter().copied())
            .map(str::to_owned)
            .collect()
    }

    fn terminal(&self, args: &[&str]) -> Client {
        self.terminal_in(&self.home.path().join("project"), args)
    }

    fn terminal_in(&self, cwd: &std::path::Path, args: &[&str]) -> Client {
        let home = self.home.path().to_string_lossy();
        let mut config = SessionConfig::new()
            .with_command_args(Self::args(args))
            .with_agent_env("NORI_HOME", home.as_ref())
            .with_agent_env("CODEX_HOME", home.as_ref())
            .with_agent_env("NORI_NO_ANALYTICS", "1")
            .with_agent_env("MOCK_AGENT_RESPONSE", ANSWER)
            .with_extra_path(self.binary.parent().unwrap().to_path_buf());
        if self.steering {
            config = config.with_agent_env("MOCK_AGENT_STEERING", "1");
        }
        config.cwd = Some(cwd.to_path_buf());
        Client(TuiSession::spawn_with_config(40, 120, config).unwrap())
    }

    fn command(&self, args: &[&str], timeout: Duration) -> Output {
        let mut stdout = tempfile::tempfile().unwrap();
        let mut stderr = tempfile::tempfile().unwrap();
        let path = std::env::join_paths(
            std::iter::once(self.binary.parent().unwrap().to_path_buf()).chain(
                std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
            ),
        )
        .unwrap();
        let mut child = Command::new(&self.binary)
            .args(Self::args(args))
            .current_dir(self.home.path().join("project"))
            .env("NORI_HOME", self.home.path())
            .env("CODEX_HOME", self.home.path())
            .env("PATH", path)
            .env("NORI_NO_ANALYTICS", "1")
            .env("MOCK_AGENT_RESPONSE", ANSWER)
            .stdin(Stdio::null())
            .stdout(stdout.try_clone().unwrap())
            .stderr(stderr.try_clone().unwrap())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + timeout;
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let status = child.wait().unwrap();
                if !std::thread::panicking() {
                    panic!("nori {args:?} did not exit within {timeout:?}: {status}");
                }
                break status;
            }
            std::thread::sleep(Duration::from_millis(30));
        };
        stdout.seek(SeekFrom::Start(0)).unwrap();
        stderr.seek(SeekFrom::Start(0)).unwrap();
        let mut output = Output {
            status,
            stdout: Vec::new(),
            stderr: Vec::new(),
        };
        stdout.read_to_end(&mut output.stdout).unwrap();
        stderr.read_to_end(&mut output.stderr).unwrap();
        output
    }

    fn start(&self) {
        let mut terminal = self.terminal(&["host", "--port", "0"]);
        terminal
            .wait_for(|screen| screen.to_lowercase().contains("started"), TIMEOUT)
            .expect("nori host reports a ready background host");
        assert!(
            terminal.wait_for_process_exit(TIMEOUT),
            "host command must detach"
        );
        self.list();
    }

    fn list(&self) -> String {
        let output = self.command(&["host", "ls"], TIMEOUT);
        assert!(
            output.status.success(),
            "host ls failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(
            !text.contains('\u{1b}'),
            "piped host ls must be plain: {text:?}"
        );
        assert_eq!(
            text.lines()
                .next()
                .unwrap_or_default()
                .split_whitespace()
                .collect::<Vec<_>>(),
            ["ID", "AGENT", "STATUS", "AGE", "CWD", "TITLE"],
            "host ls exposes the accepted flat table"
        );
        text
    }

    fn session_id(&self) -> String {
        self.list()
            .lines()
            .nth(1)
            .expect("one session row")
            .split_whitespace()
            .next()
            .unwrap()
            .to_owned()
    }

    fn attach(&self, id: Option<&str>) -> Client {
        let mut client = match id {
            Some(id) => self.terminal(&["attach", id]),
            None => self.terminal(&["attach"]),
        };
        client
            .wait_for_text(if id.is_some() { "›" } else { "Sessions" }, TIMEOUT)
            .unwrap();
        client
    }

    fn create_session(&self) -> Client {
        let mut client = self.attach(None);
        client.send_key(Key::Escape).unwrap();
        client
            .wait_for(
                |screen| screen.contains("No session selected. Use"),
                TIMEOUT,
            )
            .unwrap();
        client.submit_input("/new").unwrap();
        client.wait_for_text("mock-model", TIMEOUT).unwrap();
        client.wait_for_text("ready", TIMEOUT).unwrap();
        client
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.command(&["host", "stop"], CLEANUP_TIMEOUT);
    }
}

struct Client(TuiSession);

impl Deref for Client {
    type Target = TuiSession;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Client {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.0.terminate_and_wait(Duration::from_secs(2));
    }
}

fn ask(client: &mut Client, prompt: &str) {
    let before = client.history_contents().matches(ANSWER).count();
    client.submit_input(prompt).unwrap();
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if client.history_contents().matches(ANSWER).count() > before {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "host did not answer {prompt:?}:\n{}",
            client.screen_contents()
        );
        client.settle(Duration::from_millis(50));
    }
    client.settle(Duration::from_millis(200));
}

fn wait_watching(client: &mut Client) {
    client
        .wait_for(
            |screen| {
                let text = screen.to_lowercase();
                (text.contains("watching") || text.contains("watcher"))
                    && text.contains("read-only")
            },
            TIMEOUT,
        )
        .expect("watcher role is visible and explicitly read-only");
}

fn wait_controlling(client: &mut Client) {
    client
        .wait_for(
            |screen| {
                let text = screen.to_lowercase();
                text.contains("controlling") || text.contains("controller")
            },
            TIMEOUT,
        )
        .expect("controller role is visible");
}

fn refusal_count(client: &mut Client) -> usize {
    client
        .history_contents()
        .to_lowercase()
        .lines()
        .filter(|line| {
            line.contains("read-only") || line.contains("not allowed") || line.contains("cannot")
        })
        .count()
}

fn wait_refusal(client: &mut Client, before: usize) {
    let deadline = Instant::now() + TIMEOUT;
    while refusal_count(client) <= before {
        assert!(
            Instant::now() < deadline,
            "watcher action must show an error:\n{}",
            client.screen_contents()
        );
        client.settle(Duration::from_millis(50));
    }
}

#[test]
fn host_starts_without_a_terminal_session_and_stops_explicitly() {
    let host = Host::new();
    host.start();
    assert_eq!(host.list().lines().count(), 1, "new host starts empty");
    let mut client = host.attach(None);
    client.wait_for_text("Sessions 0", TIMEOUT).unwrap();
    client.send_key(Key::Ctrl('d')).unwrap();
    assert!(
        client.wait_for_process_exit(TIMEOUT),
        "Ctrl+D detaches the client"
    );
    assert_eq!(
        host.list().lines().count(),
        1,
        "empty host survives detachment"
    );
    let stop = host.command(&["host", "stop"], CLEANUP_TIMEOUT);
    assert!(
        stop.status.success(),
        "{}",
        String::from_utf8_lossy(&stop.stderr)
    );
    assert!(
        !host.command(&["host", "ls"], TIMEOUT).status.success(),
        "stopped host is unavailable"
    );
}

#[test]
fn detach_and_named_reattach_keep_the_session_history_and_host_alive() {
    let host = Host::new();
    host.start();
    let mut client = host.create_session();
    ask(&mut client, "D2 durable history");
    let id = host.session_id();
    assert!(
        host.list().contains("D2 durable history"),
        "host list includes the session title"
    );
    client.send_key(Key::Ctrl('d')).unwrap();
    assert!(client.wait_for_process_exit(TIMEOUT));

    let mut list = host.attach(None);
    list.wait_for_text("D2 durable history", TIMEOUT).unwrap();
    list.send_key(Key::Enter).unwrap();
    list.wait_for_text(ANSWER, TIMEOUT)
        .expect("opening a listed session replays its output");
    list.submit_input("/quit").unwrap();
    assert!(
        list.wait_for_process_exit(TIMEOUT),
        "/quit detaches without stopping the host"
    );

    let mut reattached = host.attach(Some(&id));
    reattached
        .wait_for_text(ANSWER, TIMEOUT)
        .expect("named attach restores output");
    ask(&mut reattached, "D2 after reattach");
    assert_eq!(
        host.session_id(),
        id,
        "reattach preserves conversation identity"
    );
    reattached.submit_input("/close").unwrap();
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let table = host.list();
        if table.lines().skip(1).all(|row| row.contains("closed")) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "last session must close: {table}"
        );
        reattached.settle(Duration::from_millis(100));
    }
    assert!(
        host.command(&["host", "ls"], TIMEOUT).status.success(),
        "host outlives its last session"
    );
}

#[test]
fn two_terminals_watch_take_control_and_refuse_the_previous_controllers_prompt() {
    let host = Host::new();
    host.start();
    let mut first = host.create_session();
    ask(&mut first, "D2 shared session");
    let id = host.session_id();
    let mut second = host.attach(Some(&id));
    wait_controlling(&mut first);
    wait_watching(&mut second);
    second.wait_for_text(ANSWER, TIMEOUT).unwrap();

    let before_refusal = refusal_count(&mut second);
    let before_answers = first.history_contents().matches(ANSWER).count();
    second.send_str("D2 watcher must not run").unwrap();
    second.settle(Duration::from_millis(500));
    second.send_key(Key::Enter).unwrap();
    wait_refusal(&mut second, before_refusal);
    first.settle(Duration::from_millis(500));
    assert_eq!(
        first.history_contents().matches(ANSWER).count(),
        before_answers,
        "watcher prompt never reaches the agent"
    );

    second.send_key(Key::Ctrl('u')).unwrap();
    second.send_key(Key::Alt('t')).unwrap();
    wait_controlling(&mut second);
    wait_watching(&mut first);
    ask(&mut second, "D2 new controller");
    let deadline = Instant::now() + TIMEOUT;
    while first.history_contents().matches(ANSWER).count() <= before_answers {
        assert!(
            Instant::now() < deadline,
            "former controller must receive the new answer as a watcher:\n{}",
            first.screen_contents()
        );
        first.settle(Duration::from_millis(50));
    }

    let before_refusal = refusal_count(&mut first);
    first.send_str("D2 former controller must not run").unwrap();
    first.settle(Duration::from_millis(500));
    first.send_key(Key::Enter).unwrap();
    wait_refusal(&mut first, before_refusal);
    assert!(
        !second.wait_for_process_exit(Duration::from_millis(200)),
        "takeover preserves both connections"
    );
}

#[test]
fn watcher_cannot_answer_a_permission_request_but_controller_can() {
    let host = Host::new();
    host.start();
    let mut controller = host.create_session();
    ask(&mut controller, "D2 approval session");
    let mut watcher = host.attach(Some(&host.session_id()));
    wait_watching(&mut watcher);
    controller
        .submit_input("mock:request-permission:echo D2-permission")
        .unwrap();
    controller.wait_for_text("D2-permission", TIMEOUT).unwrap();
    watcher.wait_for_text("D2-permission", TIMEOUT).unwrap();
    let deadline = Instant::now() + TIMEOUT;
    while !host.list().contains("waiting on you") {
        assert!(
            Instant::now() < deadline,
            "permission request should block the session"
        );
        watcher.settle(Duration::from_millis(50));
        controller.settle(Duration::from_millis(50));
    }
    let before = refusal_count(&mut watcher);
    watcher.wait_out_answer_grace();
    watcher.send_key(Key::Enter).unwrap();
    wait_refusal(&mut watcher, before);
    assert!(
        host.list().contains("waiting on you"),
        "watcher answer leaves permission pending"
    );
    controller.send_key(Key::Enter).unwrap();
    controller
        .wait_for_text("Permission granted with option: allow", TIMEOUT)
        .unwrap();
    watcher
        .wait_for_text("Permission granted with option: allow", TIMEOUT)
        .expect("watcher receives the controller-approved result");
}

enum ProxyCommand {
    Pause(mpsc::Sender<()>),
    Blackhole(mpsc::Sender<()>),
    Resume,
    Stop,
}

/// A real TCP link whose outages do not affect the host or its other clients.
/// Nonblocking sockets keep the single worker responsive to teardown commands.
struct InterruptibleProxy {
    endpoint: String,
    commands: mpsc::Sender<ProxyCommand>,
    worker: Option<JoinHandle<()>>,
}

struct ForwardedConnection {
    client: TcpStream,
    host: TcpStream,
    to_host: Vec<u8>,
    to_client: Vec<u8>,
}

impl Drop for ForwardedConnection {
    fn drop(&mut self) {
        let _ = self.client.shutdown(Shutdown::Both);
        let _ = self.host.shutdown(Shutdown::Both);
    }
}

impl ForwardedConnection {
    fn forward(&mut self) -> std::io::Result<bool> {
        fn pump(
            source: &mut TcpStream,
            destination: &mut TcpStream,
            pending: &mut Vec<u8>,
        ) -> std::io::Result<bool> {
            if pending.len() < 64 * 1024 {
                let mut buffer = [0; 8192];
                match source.read(&mut buffer) {
                    Ok(0) => return Ok(false),
                    Ok(count) => pending.extend_from_slice(&buffer[..count]),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(error) => return Err(error),
                }
            }
            if !pending.is_empty() {
                match destination.write(pending) {
                    Ok(0) => return Ok(false),
                    Ok(count) => {
                        pending.drain(..count);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(error) => return Err(error),
                }
            }
            Ok(true)
        }
        Ok(pump(&mut self.client, &mut self.host, &mut self.to_host)?
            && pump(&mut self.host, &mut self.client, &mut self.to_client)?)
    }
}

impl InterruptibleProxy {
    fn new(target: SocketAddr) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("ws://{}/sessions", listener.local_addr().unwrap());
        let (commands, incoming) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut enabled = true;
            let mut forwarding = true;
            let mut connection: Option<ForwardedConnection> = None;
            loop {
                while let Ok(command) = incoming.try_recv() {
                    match command {
                        ProxyCommand::Pause(done) => {
                            enabled = false;
                            connection = None;
                            let _ = done.send(());
                        }
                        ProxyCommand::Blackhole(done) => {
                            // Keep both sockets open. Neither EOF nor a TCP reset
                            // can tell the attached client that traffic has stopped.
                            forwarding = false;
                            let _ = done.send(());
                        }
                        ProxyCommand::Resume => {
                            enabled = true;
                            forwarding = true;
                        }
                        ProxyCommand::Stop => return,
                    }
                }
                match listener.accept() {
                    Ok((client, _)) if enabled && connection.is_none() => {
                        if let Ok(host) =
                            TcpStream::connect_timeout(&target, Duration::from_millis(200))
                        {
                            client.set_nonblocking(true).unwrap();
                            host.set_nonblocking(true).unwrap();
                            connection = Some(ForwardedConnection {
                                client,
                                host,
                                to_host: Vec::new(),
                                to_client: Vec::new(),
                            });
                        }
                    }
                    Ok((client, _)) => {
                        let _ = client.shutdown(Shutdown::Both);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(error) => panic!("proxy listener: {error}"),
                }
                if forwarding
                    && let Some(active) = connection.as_mut()
                    && !matches!(active.forward(), Ok(true))
                {
                    connection = None;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        Self {
            endpoint,
            commands,
            worker: Some(worker),
        }
    }

    fn pause(&self) {
        let (done, ack) = mpsc::channel();
        self.commands.send(ProxyCommand::Pause(done)).unwrap();
        ack.recv_timeout(Duration::from_secs(1))
            .expect("proxy closed both socket directions");
    }

    fn resume(&self) {
        self.commands.send(ProxyCommand::Resume).unwrap();
    }

    fn blackhole(&self) {
        let (done, ack) = mpsc::channel();
        self.commands.send(ProxyCommand::Blackhole(done)).unwrap();
        ack.recv_timeout(Duration::from_secs(1))
            .expect("proxy stopped forwarding without closing its sockets");
    }
}

impl Drop for InterruptibleProxy {
    fn drop(&mut self) {
        let _ = self.commands.send(ProxyCommand::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[test]
fn dropped_tcp_connection_shows_reconnecting_and_replays_missed_output_once() {
    assert_reconnect_after_outage(false);
}

#[test]
fn silently_blackholed_connection_shows_reconnecting_and_replays_missed_output_once() {
    assert_reconnect_after_outage(true);
}

fn assert_reconnect_after_outage(blackhole: bool) {
    let host = Host::new();
    host.start();
    let mut controller = host.create_session();
    ask(&mut controller, "D2 before network outage");
    let id = host.session_id();
    let state: serde_json::Value =
        serde_json::from_slice(&std::fs::read(host.home.path().join("host/state.json")).unwrap())
            .unwrap();
    let target: SocketAddr = state["endpoint"]
        .as_str()
        .unwrap()
        .strip_prefix("ws://")
        .unwrap()
        .split('/')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let proxy = InterruptibleProxy::new(target);
    let token_path = host.home.path().join("host/token");
    let mut viewer = host.terminal(&[
        "attach",
        &id,
        "--host",
        &proxy.endpoint,
        "--token-file",
        token_path.to_str().unwrap(),
    ]);
    wait_watching(&mut viewer);
    viewer.wait_for_text(ANSWER, TIMEOUT).unwrap();
    assert_eq!(viewer.history_contents().matches(ANSWER).count(), 1);

    let detection_timeout = if blackhole {
        proxy.blackhole();
        Duration::from_secs(20)
    } else {
        proxy.pause();
        TIMEOUT
    };
    viewer
        .wait_for_text("RECONNECTING", detection_timeout)
        .expect("an unresponsive transport is visible even without EOF or a TCP reset");
    ask(&mut controller, "D2 output created during outage");
    assert!(
        !viewer
            .history_contents()
            .contains("D2 output created during outage")
    );
    assert_eq!(viewer.history_contents().matches(ANSWER).count(), 1);

    proxy.resume();
    wait_watching(&mut viewer);
    let deadline = Instant::now() + TIMEOUT;
    while viewer.history_contents().matches(ANSWER).count() < 2 {
        assert!(
            Instant::now() < deadline,
            "reconnect must deliver missed output:\n{}",
            viewer.screen_contents()
        );
        viewer.settle(Duration::from_millis(50));
    }
    assert!(!viewer.screen_contents().contains("RECONNECTING"));
    assert_eq!(
        viewer.history_contents().matches(ANSWER).count(),
        2,
        "resume cursor must not replay already displayed answers"
    );
    assert_eq!(
        viewer
            .history_contents()
            .matches("D2 output created during outage")
            .count(),
        1,
        "missed prompt is replayed once"
    );

    viewer.send_key(Key::Alt('t')).unwrap();
    wait_controlling(&mut viewer);
    wait_watching(&mut controller);
    ask(&mut viewer, "D2 prompt after network recovery");
    assert_eq!(
        viewer.history_contents().matches(ANSWER).count(),
        3,
        "recovered connection carries new prompts without duplicating replay"
    );
}

#[test]
fn same_port_host_restart_stops_reconnecting_and_explains_session_loss() {
    let host = Host::new();
    host.start();
    let mut client = host.create_session();
    ask(&mut client, "Session before restart");
    let state: serde_json::Value =
        serde_json::from_slice(&std::fs::read(host.home.path().join("host/state.json")).unwrap())
            .unwrap();
    let address: SocketAddr = state["endpoint"]
        .as_str()
        .unwrap()
        .trim_start_matches("ws://")
        .split('/')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let old_token = std::fs::read(host.home.path().join("host/token")).unwrap();
    let stopped = host.command(&["host", "stop"], CLEANUP_TIMEOUT);
    assert!(stopped.status.success());
    client.wait_for_text("RECONNECTING", TIMEOUT).unwrap();
    let started = host.command(&["host", "--port", &address.port().to_string()], TIMEOUT);
    assert!(
        started.status.success(),
        "{}",
        String::from_utf8_lossy(&started.stderr)
    );
    assert_ne!(
        std::fs::read(host.home.path().join("host/token")).unwrap(),
        old_token
    );
    client.wait_for_text("Host restarted", TIMEOUT).unwrap();
    let screen = client.screen_contents();
    assert!(
        screen.contains("sessions were lost") && screen.contains("nori attach"),
        "{screen}"
    );
    assert!(
        !screen.contains("RECONNECTING") && !screen.contains("sessions continue"),
        "{screen}"
    );
    client.send_key(Key::Escape).unwrap();
    client.send_key(Key::Alt('t')).unwrap();
    client.settle(Duration::from_millis(200));
    assert!(client.screen_contents().contains("Host restarted"));
    let mut fresh = host.attach(None);
    fresh.wait_for_text("Sessions 0", TIMEOUT).unwrap();
    client.send_key(Key::Ctrl('d')).unwrap();
    assert!(client.wait_for_process_exit(TIMEOUT));
}

#[test]
fn host_list_truncates_tty_title_without_losing_piped_title() {
    let host = Host::new();
    host.start();
    let mut client = host.create_session();
    let title = "Long title aligned columns should preserve the entire title without putting continuation text into the ID column";
    ask(&mut client, title);
    assert!(host.list().contains(title));
    let mut listing = host.terminal(&["host", "ls"]);
    assert!(listing.wait_for_process_exit(TIMEOUT));
    let screen = listing.screen_contents();
    let row = screen
        .lines()
        .find(|line| line.contains("Long title"))
        .expect("session row");
    let title_cell = row.split("  mock-model").next().unwrap_or_default();
    assert!(
        title_cell.contains("Long title") && title_cell.contains('…'),
        "{screen}"
    );
    assert!(
        title_cell.trim_end().ends_with("ID column"),
        "a cut title keeps its end: {screen}"
    );
    let below = screen
        .lines()
        .skip_while(|line| !line.contains("Long title"))
        .nth(1)
        .unwrap_or_default();
    assert!(
        below.contains(" ago"),
        "the row under the title is its reply row, not a wrapped title: {screen}"
    );
}

#[test]
fn delegated_child_in_host_list_preserves_metadata_columns_and_can_be_opened() {
    let host = Host::new();
    host.start();
    let mut client = host.create_session();
    ask(&mut client, "parent host task");
    client
        .submit_input("/delegate mock-model mock:request-permission:child-column-check")
        .unwrap();
    let deadline = Instant::now() + TIMEOUT;
    let listing = loop {
        let listing = host.list();
        if listing.lines().count() >= 3 && listing.contains("waiting on you") {
            break listing;
        }
        assert!(
            Instant::now() < deadline,
            "child must start: {listing}\n{}",
            client.screen_contents()
        );
        client.settle(Duration::from_millis(100));
    };
    let rows: Vec<_> = listing.lines().skip(1).collect();
    assert_eq!(rows.len(), 2, "{listing}");
    for row in &rows {
        let id = row.get(..36).unwrap();
        assert_eq!(id.len(), 36, "{listing}");
        assert!(
            !id.starts_with(' '),
            "child hierarchy must stay in TITLE: {listing}"
        );
        assert_eq!(row.get(38..54).unwrap().trim(), "mock-model", "{listing}");
        assert!(
            ["ready", "waiting on you", "working"].contains(&row.get(56..72).unwrap().trim()),
            "STATUS must keep its column: {listing}"
        );
    }
    assert!(
        rows.iter()
            .any(|row| row.get(82..).unwrap().contains("child-column-check")),
        "{listing}"
    );
    client.wait_for_text("is waiting on you", TIMEOUT).unwrap();
    let parent_screen = client.screen_contents();
    assert!(
        parent_screen.contains("child-column-ch") && parent_screen.contains("parent host task"),
        "notice names child and parent by their tasks: {parent_screen}"
    );
    client.send_key(Key::AltDown).unwrap();
    client.wait_for_text("child-column-check", TIMEOUT).unwrap();
    client.wait_for_text("Allow", TIMEOUT).unwrap();
    client.wait_out_answer_grace();
    client.send_key(Key::Char('1')).unwrap();
    client
        .wait_for_text("Permission granted with option: allow", TIMEOUT)
        .unwrap();
}

#[test]
fn watcher_cannot_delegate_a_child() {
    let host = Host::new();
    host.start();
    let mut controller = host.create_session();
    ask(&mut controller, "delegation ownership");
    let id = host.session_id();
    let mut watcher = host.attach(Some(&id));
    wait_watching(&mut watcher);
    let before = refusal_count(&mut watcher);
    watcher
        .submit_input("/delegate mock-model watcher forbidden child")
        .unwrap();
    wait_refusal(&mut watcher, before);
    assert_eq!(
        host.list().lines().count(),
        2,
        "watcher cannot create a child"
    );
}

#[test]
fn new_sessions_start_in_the_attaching_folder_or_a_tab_completed_one() {
    let host = Host::new();
    host.start();
    let elsewhere = host.home.path().join("elsewhere");
    let third = host.home.path().join("third-folder");
    std::fs::create_dir(&elsewhere).unwrap();
    std::fs::create_dir(&third).unwrap();
    let mut client = host.terminal_in(&elsewhere, &["attach"]);
    client.wait_for_text("Sessions", TIMEOUT).unwrap();
    client.send_key(Key::Escape).unwrap();
    client
        .wait_for(
            |screen| screen.contains("No session selected. Use"),
            TIMEOUT,
        )
        .unwrap();

    client.submit_input("/new").unwrap();
    client
        .wait_for(
            |screen| {
                screen
                    .lines()
                    .nth(1)
                    .unwrap_or_default()
                    .contains("elsewhere")
            },
            TIMEOUT,
        )
        .expect("plain /new starts in the attaching terminal's folder, shown on the header");
    client.wait_for_text("ready", TIMEOUT).unwrap();

    client.type_input("/new ../thi").unwrap();
    client.send_key(Key::Tab).unwrap();
    client
        .wait_for_text("../third-folder/", TIMEOUT)
        .expect("Tab completes the folder");
    client.send_key(Key::Enter).unwrap();
    client
        .wait_for(
            |screen| {
                screen
                    .lines()
                    .nth(1)
                    .unwrap_or_default()
                    .contains("third-folder")
            },
            TIMEOUT,
        )
        .expect("the new session's header names its folder");
    ask(&mut client, "hello");

    let listing = host.list();
    for folder in [&elsewhere, &third] {
        let folder = folder.canonicalize().unwrap().display().to_string();
        assert!(listing.contains(&folder), "{listing}");
    }
    client.send_key(Key::Alt('s')).unwrap();
    client
        .wait_for_text(&format!("third-folder · {ANSWER}"), TIMEOUT)
        .expect("the list shows each session's folder before its reply");
}

#[test]
fn attached_busy_enter_reports_fallback_and_tab_explicitly_queues() {
    let host = Host::new();
    host.start();
    let mut client = host.create_session();
    client.submit_input("mock:stream-until-cancel").unwrap();
    client.wait_for_text("Streaming...", TIMEOUT).unwrap();
    client.submit_input("Chart the orchard first").unwrap();
    client
        .wait_for_text("Steering unavailable", TIMEOUT)
        .unwrap();
    client.type_input("Count the cedar trees").unwrap();
    client.send_key(Key::Tab).unwrap();
    client
        .wait_for_text("Queued for the next turn", TIMEOUT)
        .unwrap();
}

#[path = "support/steering.rs"]
mod steering;

#[test]
fn attached_enter_delivers_to_the_original_turn_then_tab_drains_once() {
    let mut host = Host::new();
    host.steering = true;
    host.start();
    let mut client = host.create_session();
    steering::exercise(&mut client);
}

#[test]
fn cancel_in_an_attached_terminal_ends_the_running_turn() {
    let host = Host::new();
    host.start();
    let mut client = host.create_session();
    client.submit_input("mock:stream-until-cancel").unwrap();
    let working = |host: &Host| {
        host.list()
            .lines()
            .nth(1)
            .is_some_and(|row| row.contains("working"))
    };
    let deadline = Instant::now() + TIMEOUT;
    while !working(&host) {
        assert!(Instant::now() < deadline, "the turn never started");
        client.settle(Duration::from_millis(50));
    }

    client.submit_input("/cancel").unwrap();

    let deadline = Instant::now() + TIMEOUT;
    while working(&host) {
        assert!(
            Instant::now() < deadline,
            "the turn kept running: {}",
            host.list()
        );
        client.settle(Duration::from_millis(50));
    }
    assert!(
        host.list()
            .lines()
            .nth(1)
            .is_some_and(|row| row.contains("ready")),
        "the session is ready for the next prompt: {}",
        host.list()
    );
}

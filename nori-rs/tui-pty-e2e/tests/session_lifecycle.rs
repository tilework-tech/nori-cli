//! Starting, closing and looping sessions beside each other, behind
//! `[features] multi_session`.

use std::net::TcpListener;
use std::time::Duration;
use std::time::Instant;

use serde_json::Value;
use serde_json::json;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TuiSession;
use tungstenite::Message;
use tungstenite::connect;

const ANSWER: &str = "alpha agent answer";
const STREAM: &str = "streaming work mock:stream-until-cancel";

fn config(extra_toml: &str) -> SessionConfig {
    SessionConfig::new()
        .with_extra_config_toml(format!("[features]\nmulti_session = true\n{extra_toml}"))
        .with_agent_env("MOCK_AGENT_RESPONSE_MOCK_MODEL", ANSWER)
        .with_agent_env("MOCK_AGENT_RESPONSE_MOCK_MODEL_ALT", "beta agent answer")
}

fn spawn_with(config: SessionConfig) -> TuiSession {
    let mut session = TuiSession::spawn_with_config(40, 110, config).expect("spawn");
    session
        .wait_for_text("›", TIMEOUT)
        .expect("TUI should start");
    std::thread::sleep(TIMEOUT_INPUT);
    session
}

/// Submit `prompt` and wait for a new copy of `answer` in the history.
fn ask(session: &mut TuiSession, prompt: &str, answer: &str) {
    let before = session.history_contents().matches(answer).count();
    session.submit_input(prompt).unwrap();
    let deadline = Instant::now() + TIMEOUT;
    while session.history_contents().matches(answer).count() <= before {
        assert!(
            Instant::now() < deadline,
            "{answer:?} should arrive for {prompt:?}:\n{}",
            session.screen_contents()
        );
        session.settle(Duration::from_millis(50));
    }
    session.settle(Duration::from_millis(300));
}

fn start_new_session(session: &mut TuiSession, sessions_after: &str) {
    session.submit_input("/new").unwrap();
    session
        .wait_for(
            |screen| screen.contains(sessions_after) && screen.contains("›"),
            TIMEOUT,
        )
        .expect("a new session should start");
    session.settle(Duration::from_millis(500));
}

fn open_list(session: &mut TuiSession, header: &str) {
    session.send_key(Key::Alt('s')).unwrap();
    session
        .wait_for_text(header, Duration::from_secs(3))
        .expect("the session list should open");
    session.settle(Duration::from_millis(200));
}

/// Whether a line of `screen` holds both `title` and `word`.
fn has_row(screen: &str, title: &str, word: &str) -> bool {
    screen
        .lines()
        .any(|line| line.contains(title) && line.contains(word))
}

#[test]
#[cfg(target_os = "linux")]
fn at_the_cap_a_new_session_is_refused_with_how_to_close_one() {
    let mut session = spawn_with(config("[tui]\nmax_sessions = 2\n"));
    ask(&mut session, "first question", ANSWER);
    start_new_session(&mut session, "2 ready");
    ask(&mut session, "second question", ANSWER);

    session.submit_input("/new").unwrap();
    session
        .wait_for_text("2 sessions are open", Duration::from_secs(5))
        .expect("/new at the cap says why it is refused");
    session.settle(Duration::from_millis(300));
    let screen = session.screen_contents();
    let message: String = screen
        .lines()
        .skip_while(|line| !line.contains("2 sessions are open"))
        .take(3)
        .collect::<Vec<_>>()
        .join(" ");
    for expected in ["max_sessions", "ctrl+d", "/close"] {
        assert!(
            message.contains(expected),
            "{expected:?} missing:\n{screen}"
        );
    }
    assert!(
        screen.contains("2 ready"),
        "no third session starts:\n{screen}"
    );
    assert!(
        screen.contains("second question"),
        "the session on screen stays:\n{screen}"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn ctrl_d_in_the_list_closes_a_session_and_frees_its_slot() {
    let mut session = spawn_with(config("[tui]\nmax_sessions = 2\n"));
    ask(&mut session, "first question", ANSWER);
    start_new_session(&mut session, "2 ready");
    ask(&mut session, "second question", ANSWER);

    open_list(&mut session, "Sessions 2");
    session.send_key(Key::Up).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Ctrl('d')).unwrap();
    session
        .wait_for(
            |screen| has_row(screen, "first question", "closed"),
            Duration::from_secs(8),
        )
        .expect("the closed session stays listed as closed");
    session.send_key(Key::Escape).unwrap();
    session
        .wait_for(
            |screen| screen.contains("1 ready") && !screen.contains("2 ready"),
            Duration::from_secs(3),
        )
        .expect("the status line stops counting the closed session");

    start_new_session(&mut session, "2 ready");
    ask(&mut session, "third question", ANSWER);
}

#[test]
#[cfg(target_os = "linux")]
fn closing_a_busy_session_asks_for_a_second_press() {
    let mut session = spawn_with(config(""));
    session.submit_input(STREAM).unwrap();
    session
        .wait_for_text("esc to interrupt", Duration::from_secs(8))
        .expect("the first session streams");
    start_new_session(&mut session, "1 working · 1 ready");

    open_list(&mut session, "Sessions 2");
    session.send_key(Key::Up).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Ctrl('d')).unwrap();
    session
        .wait_for_text("ctrl+d again to close", Duration::from_secs(3))
        .expect("a busy session needs a second press");
    assert!(has_row(
        &session.screen_contents(),
        "streaming work",
        "working"
    ));

    session.send_key(Key::Ctrl('d')).unwrap();
    session
        .wait_for(
            |screen| has_row(screen, "streaming work", "closed"),
            Duration::from_secs(8),
        )
        .expect("the second press closes the busy session");
}

#[test]
#[cfg(target_os = "linux")]
fn close_command_closes_the_session_on_screen_and_shows_the_next() {
    let mut session = spawn_with(config(""));
    ask(&mut session, "first question", ANSWER);
    start_new_session(&mut session, "2 ready");
    ask(&mut session, "second question", ANSWER);

    session.submit_input("/close").unwrap();
    session
        .wait_for(
            |screen| screen.contains("first question") && screen.contains("1 ready"),
            Duration::from_secs(8),
        )
        .expect("/close shows the next session");
    open_list(&mut session, "Sessions 2");
    let screen = session.screen_contents();
    assert!(has_row(&screen, "second question", "closed"), "{screen}");
}

#[test]
#[cfg(target_os = "linux")]
fn a_new_session_starts_beside_a_busy_one_and_takes_the_next_prompt() {
    let mut session = spawn_with(config(""));
    session.submit_input(STREAM).unwrap();
    session
        .wait_for_text("esc to interrupt", Duration::from_secs(8))
        .expect("the first session streams");

    start_new_session(&mut session, "1 working · 1 ready");
    ask(&mut session, "question beside the stream", ANSWER);
    let screen = session.screen_contents();
    assert!(!screen.contains("↳"), "nothing is queued:\n{screen}");
    assert!(screen.contains("1 working · 1 ready"), "{screen}");
}

#[test]
#[cfg(target_os = "linux")]
fn a_loop_in_the_background_stays_in_its_own_row() {
    let mut session = spawn_with(
        config("[tui]\nloop_count = 3\nmax_sessions = 2\n")
            .with_agent_env("MOCK_AGENT_DELAY_MS", "1200"),
    );
    ask(&mut session, "home question", ANSWER);
    session
        .wait_for_text("Loop iteration 3 of 3", Duration::from_secs(20))
        .expect("the first session's own loop finishes");
    session.settle(Duration::from_secs(3));

    start_new_session(&mut session, "2 ready");
    session.submit_input("loop question").unwrap();
    session.settle(Duration::from_millis(400));
    session.send_key(Key::Alt(',')).unwrap();
    session
        .wait_for(
            |screen| screen.contains("home question") && !screen.contains("loop question"),
            Duration::from_secs(5),
        )
        .expect("the jump lands on the home session");

    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        session.settle(Duration::from_millis(200));
        let screen = session.screen_contents();
        assert!(
            !screen.contains("loop question"),
            "a background iteration took over the screen:\n{screen}"
        );
        if screen.contains("2 ready") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the loop never finished:\n{screen}"
        );
    }
    open_list(&mut session, "Sessions 2");
    let screen = session.screen_contents();
    assert_eq!(
        screen.matches("loop question").count(),
        1,
        "iterations replace each other:\n{screen}"
    );
    session.send_key(Key::Escape).unwrap();
    session.send_key(Key::Alt('.')).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !session.history_contents().contains("Loop iteration 3 of 3")
        || !session.screen_contents().contains("loop question")
    {
        assert!(
            Instant::now() < deadline,
            "the loop session ran every iteration:\n{}",
            session.screen_contents()
        );
        session.settle(Duration::from_millis(100));
    }
}

#[test]
#[cfg(target_os = "linux")]
fn a_restarted_session_keeps_its_title() {
    let mut session = spawn_with(config(""));
    ask(&mut session, "alpha question", ANSWER);
    start_new_session(&mut session, "2 ready");
    ask(&mut session, "doomed question", ANSWER);
    session.submit_input("mock:exit-during-prompt").unwrap();
    session
        .wait_for_text("1 failed · 1 ready", Duration::from_secs(8))
        .expect("the agent dies");
    session.send_key(Key::Alt(',')).unwrap();
    session.settle(Duration::from_millis(300));

    open_list(&mut session, "Sessions 2");
    session.send_key(Key::Down).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Ctrl('r')).unwrap();
    session
        .wait_for_text("2 ready", Duration::from_secs(10))
        .expect("the failed session restarts");
    session.settle(Duration::from_millis(500));
    open_list(&mut session, "Sessions 2");
    let screen = session.screen_contents();
    assert!(screen.contains("doomed question"), "{screen}");
    assert!(!screen.contains("New session"), "{screen}");
}

#[test]
#[cfg(target_os = "linux")]
fn preparing_another_agent_leaves_no_line_in_the_old_session() {
    let mut session = spawn_with(config(""));
    ask(&mut session, "alpha question", ANSWER);
    session.submit_input("/agent").unwrap();
    session
        .wait_for(
            |screen| screen.contains("Select agent") || screen.contains("mock-model"),
            Duration::from_secs(8),
        )
        .expect("agent picker should open");
    session.send_key(Key::Down).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("Start a new session", Duration::from_secs(10))
        .expect("candidate session picker should open");
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text(
            "Started new conversation with agent: Mock ACP Alt",
            Duration::from_secs(10),
        )
        .expect("the alternate session should start");
    session.settle(Duration::from_millis(500));

    session.send_key(Key::Alt(',')).unwrap();
    session
        .wait_for(
            |screen| screen.contains("alpha question") && !screen.contains("Started new"),
            Duration::from_secs(5),
        )
        .expect("the jump lands on the first session");
    session.settle(Duration::from_millis(300));
    let history = session.history_contents();
    assert!(
        !history.contains("Preparing Mock ACP Alt"),
        "the first session keeps a line about the other session:\n{history}"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn a_long_title_keeps_the_request_line_on_one_row() {
    let mut session = TuiSession::spawn_with_config(
        30,
        100,
        config("[tui.hotkeys]\nnext_waiting_session = \"alt+down\"\n")
            .with_agent_env("MOCK_AGENT_PERMISSION_DELAY_MS", "1500"),
    )
    .expect("spawn");
    session
        .wait_for_text("›", TIMEOUT)
        .expect("TUI should start");
    std::thread::sleep(TIMEOUT_INPUT);
    ask(&mut session, "home", ANSWER);
    start_new_session(&mut session, "2 ready");
    session
        .submit_input(
            "please look through the whole build directory and tidy up whatever is stale \
             mock:request-permission:rm -rf target/debug",
        )
        .unwrap();
    session.settle(Duration::from_millis(200));
    session.send_key(Key::Alt(',')).unwrap();
    session
        .wait_for_text("1 waiting on you", Duration::from_secs(8))
        .expect("the request arrives in the background");
    session.settle(Duration::from_millis(300));
    let screen = session.screen_contents();
    assert!(
        has_row(&screen, "is waiting on you", "to jump"),
        "the request line should fit on one row:\n{screen}"
    );
}

fn reserve_loopback_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("reserve loopback port")
        .local_addr()
        .expect("reserved address")
        .port()
}

fn send(client: &mut tungstenite::WebSocket<impl std::io::Read + std::io::Write>, value: Value) {
    client
        .send(Message::Text(value.to_string().into()))
        .expect("send ACP message");
}

fn read_until(
    client: &mut tungstenite::WebSocket<impl std::io::Read + std::io::Write>,
    wanted: impl Fn(&Value) -> bool,
) -> Value {
    loop {
        let Message::Text(text) = client.read().expect("read ACP message") else {
            continue;
        };
        let value: Value = serde_json::from_str(text.as_str()).expect("ACP JSON");
        if wanted(&value) {
            return value;
        }
    }
}

fn read_managed_until(
    client: &mut tungstenite::WebSocket<impl std::io::Read + std::io::Write>,
    events: &mut Vec<Value>,
    wanted: impl Fn(&Value) -> bool,
) -> Value {
    loop {
        let Message::Text(text) = client.read().expect("read managed session message") else {
            continue;
        };
        let value: Value = serde_json::from_str(text.as_str()).expect("managed session JSON");
        assert!(value.get("error").is_none(), "{value}");
        if value["method"] == "_nori/session/event" {
            events.push(value["params"].clone());
        }
        if wanted(&value) {
            return value;
        }
    }
}

#[test]
#[cfg(target_os = "linux")]
fn ctrl_r_preserves_session_identity_and_its_remote_subscription() {
    let port = reserve_loopback_port();
    let mut session = spawn_with(
        config("")
            .with_arg("--remote")
            .with_arg(port.to_string())
            .with_excluded_binary("tailscale"),
    );
    ask(&mut session, "unaffected session", ANSWER);
    start_new_session(&mut session, "2 ready");
    ask(&mut session, "restart identity marker", ANSWER);
    let (mut client, _) = connect(format!("ws://127.0.0.1:{port}/sessions").as_str()).unwrap();
    if let tungstenite::stream::MaybeTlsStream::Plain(stream) = client.get_ref() {
        stream.set_read_timeout(Some(TIMEOUT)).unwrap();
    }
    let mut events = Vec::new();
    send(
        &mut client,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    );
    read_managed_until(&mut client, &mut events, |value| value["id"] == 1);
    send(
        &mut client,
        json!({"jsonrpc":"2.0","id":2,"method":"_nori/session/list","params":{}}),
    );
    let before = read_managed_until(&mut client, &mut events, |value| value["id"] == 2);
    let original_id = before["result"]["selected_session_id"]
        .as_str()
        .expect("on-screen session has an ID")
        .to_string();
    let rows_before = before["result"]["sessions"].as_array().unwrap();
    assert_eq!(rows_before.len(), 2);
    let cursor = rows_before
        .iter()
        .find(|row| row["session_id"] == original_id)
        .expect("on-screen session is listed")["latest_seq"]
        .as_i64()
        .unwrap();
    session.submit_input("/status").unwrap();
    session
        .wait_for(
            |screen| has_row(screen, "Session ID", &original_id),
            TIMEOUT,
        )
        .expect("the TUI shows the remote-addressable ID before the crash");
    send(
        &mut client,
        json!({"jsonrpc":"2.0","id":3,"method":"_nori/session/subscribe",
            "params":{"session_id":original_id,"cursors":{original_id.clone():cursor}}}),
    );
    read_managed_until(&mut client, &mut events, |value| value["id"] == 3);

    session.submit_input("mock:exit-during-prompt").unwrap();
    session
        .wait_for_text("1 failed · 1 ready", TIMEOUT)
        .unwrap();
    send(
        &mut client,
        json!({"jsonrpc":"2.0","id":4,"method":"_nori/session/list","params":{}}),
    );
    let failed = read_managed_until(&mut client, &mut events, |value| value["id"] == 4);
    assert!(
        failed["result"]["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["session_id"] == original_id && row["status"] == "failed")
    );
    let crash_cursor = failed["result"]["replay_cursors"][&original_id]
        .as_i64()
        .unwrap();

    open_list(&mut session, "Sessions 2");
    assert!(has_row(
        &session.screen_contents(),
        "restart identity marker",
        "failed"
    ));
    session.send_key(Key::Ctrl('r')).unwrap();
    session
        .wait_for_text("2 ready", TIMEOUT)
        .expect("Ctrl+R resumes the failed session");
    send(
        &mut client,
        json!({"jsonrpc":"2.0","id":5,"method":"_nori/session/list","params":{}}),
    );
    let after = read_managed_until(&mut client, &mut events, |value| value["id"] == 5);
    assert_eq!(after["result"]["selected_session_id"], original_id);
    let rows_after = after["result"]["sessions"].as_array().unwrap();
    let resumed_cursor = rows_after
        .iter()
        .find(|row| row["session_id"] == original_id)
        .unwrap()["latest_seq"]
        .as_i64()
        .unwrap();
    let ids_before = rows_before
        .iter()
        .map(|row| row["session_id"].as_str().unwrap())
        .collect::<Vec<_>>();
    let ids_after = rows_after
        .iter()
        .map(|row| row["session_id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        ids_after, ids_before,
        "restart preserves both list identities and their order"
    );

    session.submit_input("/status").unwrap();
    session
        .wait_for(
            |screen| has_row(screen, "Session ID", &original_id),
            TIMEOUT,
        )
        .expect("the TUI keeps the same visible session ID after Ctrl+R");
    let status_screen = session.screen_contents();
    assert!(
        status_screen
            .lines()
            .rev()
            .find(|line| line.contains("Session ID"))
            .is_some_and(|line| line.contains(&original_id)),
        "the newest status output keeps the original ID:\n{status_screen}"
    );
    open_list(&mut session, "Sessions 2");
    let screen = session.screen_contents();
    assert!(
        has_row(&screen, "restart identity marker", "ready"),
        "{screen}"
    );
    let list = &screen[screen.find("Sessions 2").unwrap_or(0)..];
    assert_eq!(
        list.matches("restart identity marker").count(),
        1,
        "the restarted session is listed once: {screen}"
    );
    session.send_key(Key::Escape).unwrap();
    session
        .wait_for(|screen| !screen.contains("Sessions 2"), TIMEOUT)
        .unwrap();

    send(
        &mut client,
        json!({"jsonrpc":"2.0","id":6,"method":"_nori/session/prompt",
            "params":{"session_id":original_id,
                "prompt":[{"type":"text","text":"remote prompt after restart"}]}}),
    );
    let prompted = read_managed_until(&mut client, &mut events, |value| value["id"] == 6);
    assert_eq!(prompted["result"]["session_id"], original_id);
    let request_id = &prompted["result"]["request_id"];
    let is_prompt_response = |envelope: &Value| {
        envelope["seq"]
            .as_i64()
            .is_some_and(|seq| seq > resumed_cursor)
            && envelope["event"]["source"] == "acp"
            && envelope["event"]["event"]["message_type"] == "response"
            && &envelope["event"]["event"]["request_id"] == request_id
    };
    if !events.iter().any(is_prompt_response) {
        read_managed_until(&mut client, &mut events, |value| {
            value["method"] == "_nori/session/event" && is_prompt_response(&value["params"])
        });
    }
    let response = events
        .iter()
        .find(|event| is_prompt_response(event))
        .unwrap();
    assert!(
        response["event"]["event"]["response"].get("Ok").is_some(),
        "the prompt addressed to the original ID completes successfully: {response}"
    );
    session
        .wait_for_text("remote prompt after restart", TIMEOUT)
        .unwrap();
    assert!(
        events.iter().any(|envelope| {
            envelope["seq"].as_i64().unwrap() > crash_cursor
                && envelope["event"]["source"] == "nori"
                && envelope["event"]["event"]["event_type"] == "session_started"
                && envelope["event"]["event"]["event"]["transcript_id"] == original_id
        }),
        "the existing subscription sees restart with the original identity: {events:?}"
    );
    assert!(
        events
            .iter()
            .all(|envelope| envelope["conversation_id"] == original_id)
    );
    let sequences = events
        .iter()
        .map(|envelope| envelope["seq"].as_i64().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        sequences,
        (cursor + 1..=cursor + i64::try_from(events.len()).unwrap()).collect::<Vec<_>>(),
        "the subscription continues across Ctrl+R without a gap or duplicate"
    );
    eprintln!(
        "Ctrl+R proof: before={original_id}, after={}, list order unchanged, /status ID unchanged, title retained",
        after["result"]["selected_session_id"].as_str().unwrap()
    );
    eprintln!(
        "Ctrl+R proof: existing WebSocket subscription received contiguous sequences {}..={}, including restarted SessionStarted; targeted prompt to {original_id} completed",
        cursor + 1,
        sequences.last().unwrap()
    );
}

#[test]
#[cfg(target_os = "linux")]
fn a_request_answered_by_remote_control_clears_the_approval_in_the_tui() {
    let port = reserve_loopback_port();
    let mut session = spawn_with(
        config("")
            .with_agent_env("MOCK_AGENT_PERMISSION_DELAY_MS", "1500")
            .with_arg("--remote")
            .with_arg(port.to_string())
            .with_excluded_binary("tailscale"),
    );
    ask(&mut session, "home question", ANSWER);
    start_new_session(&mut session, "2 ready");
    session.settle(Duration::from_secs(1));

    let (mut client, _) =
        connect(format!("ws://127.0.0.1:{port}/acp").as_str()).expect("connect remote control");
    if let tungstenite::stream::MaybeTlsStream::Plain(stream) = client.get_ref() {
        stream
            .set_read_timeout(Some(Duration::from_secs(15)))
            .expect("read timeout");
    }
    send(
        &mut client,
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
               "params": {"protocolVersion": 1, "clientCapabilities": {}}}),
    );
    read_until(&mut client, |value| value["id"] == json!(1));
    send(
        &mut client,
        json!({"jsonrpc": "2.0", "id": 2, "method": "session/list", "params": {}}),
    );
    let listed = read_until(&mut client, |value| value["id"] == json!(2));
    let hosted = listed["result"]["sessions"]
        .as_array()
        .expect("session catalog");
    assert_eq!(
        hosted.len(),
        1,
        "remote control hosts one session: {listed}"
    );
    let session_id = hosted[0]["sessionId"]
        .as_str()
        .expect("a hosted session")
        .to_string();
    send(
        &mut client,
        json!({"jsonrpc": "2.0", "id": 3, "method": "session/prompt",
               "params": {"sessionId": session_id,
                          "prompt": [{"type": "text",
                                      "text": "remote mock:request-permission:echo via-remote"}]}}),
    );
    session.settle(Duration::from_millis(300));
    session.send_key(Key::Alt(',')).unwrap();
    session
        .wait_for_text("1 waiting on you", Duration::from_secs(8))
        .expect("the remote prompt's request waits in the background");

    let asked = read_until(&mut client, |value| {
        value["method"] == json!("session/request_permission")
    });
    send(
        &mut client,
        json!({"jsonrpc": "2.0", "id": asked["id"],
               "result": {"outcome": {"outcome": "selected", "optionId": "allow"}}}),
    );
    session
        .wait_for(
            |screen| !screen.contains("waiting on you"),
            Duration::from_secs(8),
        )
        .expect("the remote answer clears the request");

    session.send_key(Key::Alt('.')).unwrap();
    session
        .wait_for_text("Permission granted", Duration::from_secs(8))
        .expect("the remote turn finishes on screen");
    session.settle(Duration::from_millis(300));
    let screen = session.screen_contents();
    assert!(
        !screen.contains("Would you like"),
        "the answered request is still asked:\n{screen}"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn at_the_cap_resume_preserves_the_current_session_and_explains_how_to_make_room() {
    let mut session = spawn_with(config("[tui]\nmax_sessions = 1\n"));
    ask(&mut session, "keep this conversation", ANSWER);
    session.submit_input("/resume").unwrap();
    session
        .wait_for_text("1 sessions are open", TIMEOUT)
        .expect("resume refuses before opening another picker");
    let screen = session.screen_contents();
    assert!(
        screen.contains("ctrl+d") && screen.contains("/close"),
        "{screen}"
    );
    assert!(screen.contains("keep this conversation"), "{screen}");
    ask(&mut session, "still usable after refusal", ANSWER);
}

#[test]
#[cfg(target_os = "linux")]
fn at_the_cap_a_transcript_fork_preserves_the_current_session() {
    let mut session = spawn_with(config("[tui]\nmax_sessions = 1\n"));
    ask(&mut session, "keep this parent", ANSWER);
    session.submit_input("/fork").unwrap();
    session
        .wait_for_text("1 sessions are open", TIMEOUT)
        .expect("fork refuses before preparing another process");
    let screen = session.screen_contents();
    assert!(
        screen.contains("ctrl+d") && screen.contains("/close"),
        "{screen}"
    );
    assert!(screen.contains("keep this parent"), "{screen}");
    ask(&mut session, "parent still usable", ANSWER);
}

#[test]
#[cfg(target_os = "linux")]
fn restarting_a_failed_parent_keeps_its_child_under_the_replacement() {
    let mut session = spawn_with(config(""));
    ask(&mut session, "parent question", ANSWER);
    session.submit_input("/fork").unwrap();
    session
        .wait_for_text("Select a message to rewind to", TIMEOUT)
        .unwrap();
    session.send_key(Key::Enter).unwrap();
    session.wait_for_text("2 ready", TIMEOUT).unwrap();
    session.settle(TIMEOUT_INPUT);
    session.send_key(Key::Ctrl('u')).unwrap();
    ask(&mut session, "child question", ANSWER);
    session.send_key(Key::Alt(',')).unwrap();
    session.wait_for_text("parent question", TIMEOUT).unwrap();
    session.submit_input("mock:exit-during-prompt").unwrap();
    session.wait_for_text("1 failed", TIMEOUT).unwrap();
    open_list(&mut session, "Sessions 2");
    session.send_key(Key::Ctrl('r')).unwrap();
    session.wait_for_text("2 ready", TIMEOUT).unwrap();
    open_list(&mut session, "Sessions 2");
    let screen = session.screen_contents();
    assert!(
        screen
            .lines()
            .any(|line| line.contains("└─") && line.contains("child question")),
        "{screen}"
    );
    session.send_key(Key::Escape).unwrap();
    session
        .wait_for(|screen| !screen.contains("Sessions 2"), TIMEOUT)
        .expect("the session list closes before typing into the composer");
    ask(&mut session, "restarted parent usable", ANSWER);
}

#[test]
#[cfg(target_os = "linux")]
fn legacy_remote_control_follows_the_session_on_screen() {
    let port = reserve_loopback_port();
    let mut session = spawn_with(
        config("")
            .with_arg("--remote")
            .with_arg(port.to_string())
            .with_excluded_binary("tailscale"),
    );
    ask(&mut session, "first session marker", ANSWER);
    let (mut first_client, _) = connect(format!("ws://127.0.0.1:{port}/acp").as_str()).unwrap();
    if let tungstenite::stream::MaybeTlsStream::Plain(stream) = first_client.get_ref() {
        stream
            .set_read_timeout(Some(Duration::from_secs(15)))
            .unwrap();
    }
    send(
        &mut first_client,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize",
        "params":{"protocolVersion":1,"clientCapabilities":{}}}),
    );
    read_until(&mut first_client, |value| value["id"] == json!(1));
    send(
        &mut first_client,
        json!({"jsonrpc":"2.0","id":2,"method":"session/list","params":{}}),
    );
    let first_catalog = read_until(&mut first_client, |value| value["id"] == json!(2));
    let first_id = first_catalog["result"]["sessions"][0]["sessionId"].clone();
    drop(first_client);
    start_new_session(&mut session, "2 ready");
    ask(&mut session, "second session marker", ANSWER);
    session.send_key(Key::Alt(',')).unwrap();
    session
        .wait_for_text("first session marker", TIMEOUT)
        .unwrap();
    let (mut client, _) = connect(format!("ws://127.0.0.1:{port}/acp").as_str()).unwrap();
    if let tungstenite::stream::MaybeTlsStream::Plain(stream) = client.get_ref() {
        stream
            .set_read_timeout(Some(Duration::from_secs(15)))
            .unwrap();
    }
    send(
        &mut client,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize",
        "params":{"protocolVersion":1,"clientCapabilities":{}}}),
    );
    read_until(&mut client, |value| value["id"] == json!(1));
    send(
        &mut client,
        json!({"jsonrpc":"2.0","id":2,"method":"session/list","params":{}}),
    );
    let catalog = read_until(&mut client, |value| value["id"] == json!(2));
    assert_eq!(
        catalog["result"]["sessions"][0]["sessionId"], first_id,
        "a fresh legacy connection defaults to the session on screen"
    );
    let id = catalog["result"]["sessions"][0]["sessionId"]
        .as_str()
        .unwrap();
    send(
        &mut client,
        json!({"jsonrpc":"2.0","id":3,"method":"session/prompt",
        "params":{"sessionId":id,"prompt":[{"type":"text","text":"remote selected-session marker"}]}}),
    );
    read_until(&mut client, |value| value["id"] == json!(3));
    session
        .wait_for_text("remote selected-session marker", TIMEOUT)
        .expect("the remote prompt reaches the older session on screen");
    session.send_key(Key::Alt('.')).unwrap();
    session
        .wait_for_text("second session marker", TIMEOUT)
        .unwrap();
    assert!(
        !session
            .history_contents()
            .contains("remote selected-session marker")
    );
}

#[test]
#[cfg(target_os = "linux")]
fn managed_remote_endpoint_requires_multi_session() {
    for multi_session in [false, true] {
        let port = reserve_loopback_port();
        let settings = if multi_session {
            config("")
        } else {
            SessionConfig::new()
        };
        let mut session = spawn_with(
            settings
                .with_arg("--remote")
                .with_arg(port.to_string())
                .with_excluded_binary("tailscale"),
        );
        session
            .wait_for_text("ACP URLs:", TIMEOUT)
            .expect("listener enabled");
        let connected = connect(format!("ws://127.0.0.1:{port}/sessions").as_str());
        assert_eq!(
            connected.is_ok(),
            multi_session,
            "endpoint exposure follows the feature flag"
        );
    }
}

#[test]
#[cfg(target_os = "linux")]
fn a_same_process_head_fork_is_allowed_at_the_cap_but_a_transcript_fork_is_not() {
    let mut session = spawn_with(
        config("[tui]\nmax_sessions = 1\n").with_agent_env("MOCK_AGENT_SUPPORT_SESSION_FORK", "1"),
    );
    ask(&mut session, "original question", ANSWER);
    session.submit_input("/fork").unwrap();
    session
        .wait_for_text("Branch from current point", TIMEOUT)
        .unwrap();
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("Session forked.", TIMEOUT)
        .expect("head fork shares the existing process");
    ask(&mut session, "after head fork", ANSWER);
    session.submit_input("/fork").unwrap();
    session
        .wait_for_text("Branch from current point", TIMEOUT)
        .unwrap();
    session.send_key(Key::Down).unwrap();
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("1 sessions are open", TIMEOUT)
        .expect("a transcript fork still needs another process slot");
    ask(&mut session, "still here after refused fork", ANSWER);
}

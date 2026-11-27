//! `/agent` (or `/agents`) in `nori attach` lists the ACP agent servers the
//! connected host can start, as that host reports them, and starts or
//! switches to the chosen one there. An unprompted session changes agent in
//! place; a session that has been prompted keeps its conversation and the
//! chosen agent starts in a new session beside it.
#![cfg(target_os = "linux")]
#[path = "support/agent_host.rs"]
mod agent_host;
use agent_host::*;
use tui_pty_e2e::Key;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TuiSession;

/// The description line under `agent`'s row in the picker.
fn note_of(tui: &TuiSession, agent: &str) -> String {
    let screen = tui.screen_contents();
    let mut lines = screen
        .lines()
        .skip_while(|line| !line.contains("Choose an agent"))
        .skip_while(|line| !line.contains(agent));
    lines.next();
    lines
        .next()
        .unwrap_or_default()
        .trim_matches(|c: char| c.is_whitespace() || c == '▏' || c == '▕')
        .to_owned()
}

fn open_picker(tui: &mut TuiSession, command: &str) {
    ask(tui, command);
    tui.wait_for_text("Choose an agent", TIMEOUT).unwrap();
}

fn new_amber_session(host: &AgentHost, tui: &mut TuiSession) {
    ask(tui, "/new");
    host.wait_for_trace("amber", "session/new -");
    tui.wait_for(|screen| !screen.contains("No session selected"), TIMEOUT)
        .expect("the new session is on screen");
}

/// How many times `agent` received `method`.
fn received(host: &AgentHost, agent: &str, method: &str) -> usize {
    host.trace(agent)
        .iter()
        .filter(|line| line.split_whitespace().next() == Some(method))
        .count()
}

/// The ID and agent columns of each `host ls` row.
fn rows(host: &AgentHost) -> Vec<(String, String, String)> {
    host.sessions()
        .iter()
        .map(|row| {
            let mut columns = row.split_whitespace().map(str::to_owned);
            (
                columns.next().unwrap(),
                columns.next().unwrap(),
                columns.next().unwrap(),
            )
        })
        .collect()
}

#[test]
fn a_host_with_no_sessions_lists_its_agents_and_starts_the_chosen_one() {
    let host = AgentHost::new(&[]);
    host.start();
    let mut attached = host.attach();
    let tui = &mut attached.terminal;
    close_list(tui);

    open_picker(tui, "/agents");
    for agent in ["Amber", "Cedar"] {
        assert!(
            focus(tui, "Choose an agent", agent),
            "the host lists {agent}"
        );
    }
    assert!(
        note_of(tui, "Cedar").starts_with("Adapter not installed"),
        "Cedar says why it cannot start: {:?}",
        note_of(tui, "Cedar")
    );
    assert!(
        focus(tui, "Choose an agent", "Birch"),
        "the picker focuses Birch"
    );
    tui.send_key(Key::Enter).unwrap();
    tui.wait_for_text("birch · ready", TIMEOUT).unwrap();
    ask(tui, "whoami");
    tui.wait_for_text("birch is answering (1).", TIMEOUT)
        .unwrap();
    let sessions = rows(&host);
    assert_eq!(sessions.len(), 1, "one session: {sessions:?}");
    assert_eq!(sessions[0].1, "birch");
    assert_eq!(received(&host, "birch", "session/new"), 1);
    assert_eq!(received(&host, "amber", "session/new"), 0);
}

#[test]
fn an_unprompted_session_changes_agent_in_place() {
    let host = AgentHost::new(&[]);
    host.start();
    let mut attached = host.attach();
    let tui = &mut attached.terminal;
    close_list(tui);
    new_amber_session(&host, tui);
    let before = rows(&host);
    assert_eq!(before.len(), 1, "{before:?}");
    let id = before[0].0.clone();

    open_picker(tui, "/agent");
    assert!(focus(tui, "Choose an agent", "Birch"));
    tui.send_key(Key::Enter).unwrap();
    host.wait_for_trace("birch", "session/new -");
    ask(tui, "whoami");
    tui.wait_for_text("birch is answering (1).", TIMEOUT)
        .unwrap();

    let after = rows(&host);
    assert_eq!(after.len(), 1, "still one session: {after:?}");
    assert_eq!(after[0].0, id, "the same session");
    assert_eq!(after[0].1, "birch", "now on Birch");
    assert_eq!(received(&host, "birch", "session/new"), 1);
}

#[test]
fn a_prompted_session_keeps_its_conversation_and_the_new_agent_starts_beside_it() {
    let host = AgentHost::new(&[]);
    host.start();
    let mut attached = host.attach();
    let tui = &mut attached.terminal;
    close_list(tui);
    new_amber_session(&host, tui);
    ask(tui, "whoami");
    tui.wait_for_text("amber is answering (1).", TIMEOUT)
        .unwrap();

    open_picker(tui, "/agent");
    assert!(focus(tui, "Choose an agent", "Birch"));
    tui.send_key(Key::Enter).unwrap();
    host.wait_for_trace("birch", "session/new -");
    tui.wait_for_text("Started a new session with Birch", TIMEOUT)
        .unwrap();
    ask(tui, "whoami");
    tui.wait_for_text("birch is answering (1).", TIMEOUT)
        .unwrap();

    let mut sessions = rows(&host);
    sessions.sort_by(|a, b| a.1.cmp(&b.1));
    let agents: Vec<(&str, &str)> = sessions
        .iter()
        .map(|(_, agent, status)| (agent.as_str(), status.as_str()))
        .collect();
    assert_eq!(
        agents.len(),
        2,
        "both sessions are on the host: {sessions:?}"
    );
    assert_eq!(agents[0].0, "amber");
    assert_eq!(agents[1].0, "birch");
    assert!(
        agents.iter().all(|(_, status)| *status != "closed"),
        "the Amber conversation keeps running: {sessions:?}"
    );
    assert_eq!(received(&host, "amber", "session/close"), 0);
}

#[test]
fn an_agent_the_host_cannot_start_starts_nothing() {
    let host = AgentHost::new(&[]);
    host.start();
    let mut attached = host.attach();
    let tui = &mut attached.terminal;
    close_list(tui);

    open_picker(tui, "/agent");
    assert!(
        focus(tui, "Choose an agent", "Cedar"),
        "the picker focuses Cedar"
    );
    tui.send_key(Key::Enter).unwrap();
    tui.wait_for_text("Cedar cannot start on this host", TIMEOUT)
        .unwrap();
    tui.press(
        Key::Escape,
        |screen| !screen.contains("Choose an agent"),
        "the picker closes",
    )
    .unwrap();
    assert_eq!(rows(&host), Vec::new());
}

/// Open the only session from the list a fresh attach starts with.
fn open_only_session(tui: &mut TuiSession) {
    tui.wait_for_text("Sessions", TIMEOUT).unwrap();
    tui.settle(PAUSE);
    tui.send_key(Key::Enter).unwrap();
    tui.wait_for(|screen| !screen.contains("Session  "), TIMEOUT)
        .expect("the session opens");
}

#[test]
fn a_watcher_keeps_a_session_whose_agent_changes_in_place() {
    let host = AgentHost::new(&[]);
    host.start();
    let mut controller = host.attach();
    close_list(&mut controller.terminal);
    new_amber_session(&host, &mut controller.terminal);
    let mut watcher = host.attach();
    open_only_session(&mut watcher.terminal);
    watcher.terminal.wait_for_text("watching", TIMEOUT).unwrap();

    open_picker(&mut controller.terminal, "/agent");
    assert!(focus(&mut controller.terminal, "Choose an agent", "Birch"));
    controller.terminal.send_key(Key::Enter).unwrap();
    host.wait_for_trace("birch", "session/new -");
    ask(&mut controller.terminal, "whoami");
    controller
        .terminal
        .wait_for_text("birch is answering (1).", TIMEOUT)
        .unwrap();

    watcher
        .terminal
        .wait_for_text("birch is answering (1).", TIMEOUT)
        .expect("the watcher still follows the session");
    assert!(
        !watcher.terminal.screen_contents().contains("Session ended"),
        "the watcher was not told the session ended"
    );
}

#[test]
fn a_terminal_attaching_after_an_in_place_change_opens_the_running_session() {
    let host = AgentHost::new(&[]);
    host.start();
    let mut first = host.attach();
    let tui = &mut first.terminal;
    close_list(tui);
    new_amber_session(&host, tui);
    open_picker(tui, "/agent");
    assert!(focus(tui, "Choose an agent", "Birch"));
    tui.send_key(Key::Enter).unwrap();
    host.wait_for_trace("birch", "session/new -");
    tui.wait_for_text("This session now runs Birch.", TIMEOUT)
        .unwrap();
    drop(first);

    let mut later = host.attach();
    let tui = &mut later.terminal;
    open_only_session(tui);
    tui.settle(PAUSE * 2);
    assert!(
        !tui.screen_contents().contains("Session ended"),
        "the replayed restart does not end the session here"
    );
    ask(tui, "whoami");
    tui.wait_for_text("birch is answering (1).", TIMEOUT)
        .unwrap();
}

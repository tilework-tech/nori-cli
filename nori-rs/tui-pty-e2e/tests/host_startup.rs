//! Plain `nori` starts its first session through the host it owns: the host
//! prepares the default ACP agent server and starts the session on it, so a
//! terminal attached to any host can start the same way.
#![cfg(unix)]
#[path = "support/two_agents.rs"]
mod two_agents;
use tui_pty_e2e::TIMEOUT;
use two_agents::*;

/// The host prepared the agent before it started the session.
fn started_through_the_host(tui: &tui_pty_e2e::TuiSession) {
    let requests = wait_for_host_request(tui, "session/new");
    let prepare = requests
        .iter()
        .position(|method| method == "_nori/host/prepare_agent");
    let new = requests.iter().position(|method| method == "session/new");
    assert!(
        matches!((prepare, new), (Some(prepare), Some(new)) if prepare < new),
        "{requests:?}"
    );
}

fn prompts(home: &std::path::Path, pid: &str) -> usize {
    std::fs::read_to_string(home.join("fixture-traffic-amber"))
        .unwrap()
        .lines()
        .filter(|line| line.starts_with(&format!("{pid} session/prompt")))
        .count()
}

#[test]
fn startup_activates_its_session_through_the_owned_host() {
    let (home, mut tui) = launch(&[], &[]);
    started_through_the_host(&tui);
    ask(&mut tui, "whoami");
    tui.wait_for_text("amber is answering (1).", TIMEOUT)
        .unwrap();
    assert_eq!(
        trace(home.path(), "amber"),
        [
            "initialize -",
            "session/list -",
            "session/new -",
            "session/prompt 0"
        ]
    );
}

#[test]
fn a_prompt_typed_while_the_agent_prepares_reaches_the_startup_session_once() {
    let (home, mut tui) = launch(&["fixture-hold-init"], &[]);
    let held = home.path().join("fixture-init-held");
    tui.wait_for(|_| held.exists(), TIMEOUT).unwrap();
    ask(&mut tui, "whoami");
    std::thread::sleep(PAUSE);
    assert!(
        !trace(home.path(), "amber")
            .iter()
            .any(|method| method.starts_with("session/")),
        "nothing reaches the server before it initializes"
    );
    std::fs::remove_file(home.path().join("fixture-hold-init")).unwrap();
    tui.wait_for_text("amber is answering (1).", TIMEOUT)
        .unwrap();
    started_through_the_host(&tui);
    ask(&mut tui, "whoami");
    tui.wait_for_text("amber is answering (2).", TIMEOUT)
        .unwrap();
    let amber = pids(home.path(), "amber");
    assert_eq!(amber.len(), 1, "one server for startup");
    assert_eq!(prompts(home.path(), &amber[0]), 2);
}

#[test]
fn a_positional_prompt_starts_through_the_owned_host_once() {
    let (home, mut tui) = launch(&[], &["whoami"]);
    tui.wait_for_text("amber is answering (1).", TIMEOUT)
        .unwrap();
    started_through_the_host(&tui);
    ask(&mut tui, "whoami");
    tui.wait_for_text("amber is answering (2).", TIMEOUT)
        .unwrap();
    let amber = pids(home.path(), "amber");
    assert_eq!(amber.len(), 1);
    assert_eq!(prompts(home.path(), &amber[0]), 2);
}

#[test]
fn a_refused_startup_session_reports_the_agent_and_the_next_prompt_retries() {
    let (home, mut tui) = launch(&["fixture-refuse-new"], &[]);
    tui.wait_for_text("new session unavailable", TIMEOUT)
        .unwrap();
    started_through_the_host(&tui);
    std::thread::sleep(PAUSE);
    assert_eq!(
        trace(home.path(), "amber")
            .iter()
            .filter(|method| method.starts_with("session/new"))
            .count(),
        1,
        "a refusal is not retried on its own"
    );
    ask(&mut tui, "whoami");
    tui.wait_for_text("amber is answering (1).", TIMEOUT)
        .unwrap();
    let requests = host_requests(&tui);
    assert_eq!(
        requests
            .iter()
            .filter(|method| *method == "session/new")
            .count(),
        2,
        "the retry also goes through the host: {requests:?}"
    );
    let amber = pids(home.path(), "amber");
    assert_eq!(prompts(home.path(), amber.last().unwrap()), 1);
}

#[test]
fn resume_while_the_agent_prepares_restores_on_the_startup_server() {
    let (home, mut tui) = launch(&["fixture-hold-init"], &[]);
    let held = home.path().join("fixture-init-held");
    tui.wait_for(|_| held.exists(), TIMEOUT).unwrap();
    ask(&mut tui, "/resume");
    std::fs::remove_file(home.path().join("fixture-hold-init")).unwrap();
    tui.wait_for_text("saved-1", TIMEOUT).unwrap();
    tui.send_key(tui_pty_e2e::Key::Down).unwrap();
    std::thread::sleep(PAUSE);
    tui.send_key(tui_pty_e2e::Key::Enter).unwrap();
    tui.wait_for_text("saved input", TIMEOUT).unwrap();
    ask(&mut tui, "whoami");
    tui.wait_for_text("amber is answering (1).", TIMEOUT)
        .unwrap();
    assert_eq!(
        trace(home.path(), "amber"),
        [
            "initialize -",
            "session/list -",
            "session/load saved-1",
            "session/prompt saved-1"
        ],
        "one server lists and restores, and no session is started on it"
    );
    let requests = host_requests(&tui);
    assert_eq!(
        requests
            .iter()
            .filter(|method| *method == "_nori/host/prepare_agent")
            .count(),
        1,
        "{requests:?}"
    );
}

#[test]
fn a_positional_prompt_whose_session_is_refused_returns_to_the_composer_unsent() {
    let (home, mut tui) = launch(&["fixture-refuse-new"], &["whoami"]);
    tui.wait_for_text("new session unavailable", TIMEOUT)
        .unwrap();
    tui.wait_for(
        |screen| screen.lines().any(|line| line.trim() == "› whoami"),
        TIMEOUT,
    )
    .unwrap();
    assert!(
        !trace(home.path(), "amber")
            .iter()
            .any(|method| method.starts_with("session/prompt")),
        "the refused session's prompt is never sent"
    );
    std::thread::sleep(PAUSE);
    tui.send_key(tui_pty_e2e::Key::Enter).unwrap();
    tui.wait_for_text("amber is answering (1).", TIMEOUT)
        .unwrap();
}

#[test]
fn a_positional_prompt_start_writes_one_welcome_card() {
    let (_home, mut tui) = launch(&[], &["whoami"]);
    tui.wait_for_text("amber is answering (1).", TIMEOUT)
        .unwrap();
    tui.wait_for_text("Amber options", TIMEOUT).unwrap();
    let screen = tui.screen_contents();
    assert_eq!(screen.matches("Nori CLI v").count(), 1, "{screen}");
}

#[test]
fn a_prompt_typed_while_resume_prepares_reaches_the_restored_session() {
    let (home, mut tui) = launch(&["fixture-hold-init"], &[]);
    let held = home.path().join("fixture-init-held");
    tui.wait_for(|_| held.exists(), TIMEOUT).unwrap();
    ask(&mut tui, "/resume");
    ask(&mut tui, "whoami");
    std::fs::remove_file(home.path().join("fixture-hold-init")).unwrap();
    tui.wait_for_text("saved-1", TIMEOUT).unwrap();
    tui.send_key(tui_pty_e2e::Key::Down).unwrap();
    std::thread::sleep(PAUSE);
    tui.send_key(tui_pty_e2e::Key::Enter).unwrap();
    tui.wait_for_text("amber is answering (1).", TIMEOUT)
        .unwrap();
    assert_eq!(
        trace(home.path(), "amber"),
        [
            "initialize -",
            "session/list -",
            "session/load saved-1",
            "session/prompt saved-1"
        ]
    );
}

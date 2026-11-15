//! Delegation traverses the real manager, child process, and terminal.
use std::time::Duration;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TuiSession;
use tui_pty_e2e::normalize_for_input_snapshot;

fn config(enabled: bool) -> SessionConfig {
    SessionConfig::new()
        .with_extra_config_toml(format!("[features]\nmulti_session = {enabled}\n"))
        .with_agent_env("MOCK_AGENT_RESPONSE_MOCK_MODEL", "parent final answer")
        .with_agent_env("MOCK_AGENT_RESPONSE_MOCK_MODEL_ALT", "child final answer")
}

fn spawn_fresh(rows: u16, cols: u16) -> TuiSession {
    let mut session = TuiSession::spawn_with_config(rows, cols, config(true)).unwrap();
    session
        .wait_for_text("Mock Default Model", TIMEOUT)
        .unwrap();
    session
}

fn spawn(enabled: bool) -> TuiSession {
    let mut session = TuiSession::spawn_with_config(40, 130, config(enabled)).unwrap();
    session
        .wait_for_text("Mock Default Model", TIMEOUT)
        .unwrap();
    session.submit_input("parent task identity").unwrap();
    session
        .wait_for_text("parent final answer", TIMEOUT)
        .unwrap();
    session
}

#[test]
fn same_kind_child_starts_and_returns_without_approval_or_exiting_terminal() {
    let mut session = spawn(true);
    session
        .submit_input("/delegate mock-model inspect the parser")
        .unwrap();
    session
        .wait_for(
            |screen| screen.contains("result from mock-model") && screen.contains(" · done"),
            TIMEOUT,
        )
        .unwrap();
    assert!(
        !session
            .screen_contents()
            .contains("start a mock-model-alt session")
    );
    session.submit_input("/sessions").unwrap();
    session
        .wait_for(
            |screen| screen.contains("Sessions 2") && screen.contains("└─ · inspect the parser"),
            TIMEOUT,
        )
        .unwrap();
    session.send_key(Key::Down).unwrap();
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for(
            |screen| screen.contains("inspect the parser") && screen.contains("alt + up to parent"),
            TIMEOUT,
        )
        .unwrap();
    session
        .wait_for_text("parent final answer", TIMEOUT)
        .unwrap();
    session.send_key(Key::Alt(',')).unwrap();
    session
        .wait_for_text("parent task identity", TIMEOUT)
        .unwrap();
    let before = session
        .history_contents()
        .matches("parent final answer")
        .count();
    session.submit_input("still usable").unwrap();
    let deadline = std::time::Instant::now() + TIMEOUT;
    while session
        .history_contents()
        .matches("parent final answer")
        .count()
        <= before
    {
        assert!(
            std::time::Instant::now() < deadline,
            "parent must answer after child completion"
        );
        session.settle(Duration::from_millis(50));
    }
}

#[test]
fn different_kind_child_needs_start_and_its_permission_stays_in_child() {
    let mut session = spawn(true);
    session
        .submit_input("/delegate mock-model-alt mock:request-permission:write-child-file")
        .unwrap();
    session
        .wait_for_text("start a mock-model-alt session", TIMEOUT)
        .unwrap();
    assert!(
        !session
            .screen_contents()
            .contains("Would you like to allow")
    );
    session.send_key(Key::Char('1')).unwrap();
    session
        .wait_for(
            |screen| screen.contains("waiting on you") && screen.contains("parent"),
            TIMEOUT,
        )
        .unwrap();
    assert!(
        !session
            .screen_contents()
            .contains("Would you like to allow"),
        "child permission must not open in parent"
    );
    session.send_key(Key::AltDown).unwrap();
    session
        .wait_for_text("Would you like to allow", TIMEOUT)
        .unwrap();
    session.wait_for_text("write-child-file", TIMEOUT).unwrap();
    session.send_key(Key::Char('1')).unwrap();
    session
        .wait_for_text("Permission granted with option: allow", TIMEOUT)
        .unwrap();
    session.send_key(Key::Alt(',')).unwrap();
    session
        .wait_for(
            |screen| screen.contains("result from mock-model") && screen.contains(" · done"),
            TIMEOUT,
        )
        .unwrap();
    session
        .wait_for(
            |screen| {
                screen.contains("parent task identity")
                    && screen.contains("Permission granted with option: allow")
            },
            TIMEOUT,
        )
        .expect("attributed result is visible in parent after child redraw");
}

#[test]
fn declining_different_kind_does_not_start_child() {
    let mut session = spawn(true);
    session
        .submit_input("/delegate mock-model-alt inspect parser without starting a child")
        .unwrap();
    session
        .wait_for_text("start a mock-model-alt session", TIMEOUT)
        .unwrap();
    session.send_key(Key::Char('2')).unwrap();
    session
        .wait_for_text("The user declined this delegation.", TIMEOUT)
        .unwrap();
    session
        .wait_for_text("Delegation continuation received:", TIMEOUT)
        .unwrap();
    session
        .wait_for(
            |screen| {
                !screen.contains("Working") && !screen.contains("start a mock-model-alt session")
            },
            TIMEOUT,
        )
        .unwrap();
    session.submit_input("/sessions").unwrap();
    assert!(!session.screen_contents().contains("child final answer"));
    session.wait_for_text("Sessions 1", TIMEOUT).unwrap();
}

#[test]
fn delegation_is_unavailable_without_multi_session() {
    let mut session = spawn(false);
    session
        .submit_input("/delegate mock-model-alt should not run")
        .unwrap();
    session
        .wait_for_text("Unrecognized command '/delegate'", TIMEOUT)
        .unwrap();
    assert!(
        !session
            .screen_contents()
            .contains("start a mock-model-alt session")
    );
}

#[test]
fn delegation_reads_clearly_from_request_to_result() {
    let mut session = spawn(true);
    session
        .submit_input("/delegate mock-model-alt mock:request-permission:write-child-file")
        .unwrap();
    session
        .wait_for_text("start a mock-model-alt session", TIMEOUT)
        .unwrap();
    session.settle(Duration::from_millis(200));
    insta::assert_snapshot!(
        "delegation_approval_prompt",
        normalize_for_input_snapshot(session.screen_contents())
    );

    session.send_key(Key::Char('1')).unwrap();
    session
        .wait_for(
            |screen| {
                screen.contains("delegated to mock-model-alt")
                    && screen.contains(
                        "parent task identity › mock:request-permission:write-child-file is waiting on you",
                    )
            },
            TIMEOUT,
        )
        .expect("the parent names both sessions while the child waits");
    session.settle(Duration::from_millis(200));
    let parent = session.screen_contents();
    let request_line = parent
        .lines()
        .find(|line| line.contains("● ") && line.contains("is waiting on you"))
        .unwrap_or_else(|| panic!("request line missing:\n{parent}"));
    assert!(request_line.contains("alt + down"), "{parent}");
    assert!(
        !regex::Regex::new("[0-9a-f]{8}-[0-9a-f]{4}")
            .unwrap()
            .is_match(&parent),
        "sessions are named, not identified by raw IDs:\n{parent}"
    );

    session.send_key(Key::AltDown).unwrap();
    session
        .wait_for_text("Would you like to allow", TIMEOUT)
        .unwrap();
    session
        .wait_for_text("alt + up to parent", TIMEOUT)
        .expect("the child shows the way back to its parent");

    session.send_key(Key::Char('1')).unwrap();
    session
        .wait_for_text("Permission granted with option: allow", TIMEOUT)
        .unwrap();
    session.send_key(Key::AltUp).unwrap();
    session
        .wait_for(
            |screen| {
                screen.contains("result from mock-model-alt")
                    && screen.contains("parent task identity")
            },
            TIMEOUT,
        )
        .expect("the result block names the child in the parent");
    session
        .wait_for_text("Delegation continuation received", TIMEOUT)
        .unwrap();
    assert!(
        session.history_contents().contains("returns to mock-model"),
        "the result block says where it goes"
    );

    session.submit_input("/sessions").unwrap();
    session
        .wait_for(|screen| screen.contains("└─ "), TIMEOUT)
        .expect("the child is listed under its parent");
    session.settle(Duration::from_millis(200));
    let screen = session.screen_contents();
    assert!(!screen.contains("(parent"), "{screen}");
    // Only the list itself; the transcript above it is covered elsewhere.
    let list = screen
        .lines()
        .skip_while(|line| !line.contains("Sessions 2"))
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!(
        "delegation_session_tree",
        normalize_for_input_snapshot(list)
    );
}

/// Start a different-kind child that waits on a permission, and approve it.
fn start_waiting_child(session: &mut TuiSession) {
    session
        .submit_input("/delegate mock-model-alt mock:request-permission:write-child-file")
        .unwrap();
    session
        .wait_for_text("start a mock-model-alt session", TIMEOUT)
        .unwrap();
    session.send_key(Key::Char('1')).unwrap();
    session
        .wait_for_text("is waiting on you", TIMEOUT)
        .expect("the parent says the child waits");
    session.settle(Duration::from_millis(200));
}

#[test]
fn alt_down_reaches_a_waiting_child_even_with_a_draft_in_the_parent() {
    let mut session = spawn(true);
    start_waiting_child(&mut session);
    session.type_input("my unsent draft").unwrap();
    session
        .wait_for_text("my unsent draft", TIMEOUT)
        .expect("the draft is in the parent's prompt");
    session.send_key(Key::AltDown).unwrap();
    session
        .wait_for_text("Would you like to allow", TIMEOUT)
        .expect("alt + down opens the waiting child");
    assert!(
        !session.screen_contents().contains("my unsent draft"),
        "the draft stays with the parent:\n{}",
        session.screen_contents()
    );
    session.send_key(Key::Char('1')).unwrap();
    session
        .wait_for_text("Permission granted with option: allow", TIMEOUT)
        .unwrap();
    session.send_key(Key::AltUp).unwrap();
    session
        .wait_for_text("Delegation continuation received", TIMEOUT)
        .expect("the parent gets the child's result");
    session
        .wait_for_text("my unsent draft", TIMEOUT)
        .expect("the parent's draft comes back with it");
    session.settle(Duration::from_millis(300));
    let screen = session.screen_contents();
    let lines = screen.lines().collect::<Vec<_>>();
    let draft = lines
        .iter()
        .rposition(|line| line.contains("my unsent draft"))
        .unwrap_or_else(|| panic!("draft missing:\n{screen}"));
    let last_answer = lines
        .iter()
        .rposition(|line| line.contains("Delegation continuation received"))
        .expect("the parent's answer to the result");
    assert!(
        draft > last_answer && screen.matches("my unsent draft").count() == 1,
        "the draft is still unsent, in the parent's prompt:\n{screen}"
    );
}

#[test]
fn alt_up_in_a_top_level_session_leaves_the_prompt_alone() {
    let mut session = spawn(true);
    session.settle(Duration::from_millis(300));
    let before = session.screen_contents();
    session.send_key(Key::AltUp).unwrap();
    // A typed sentinel shows that the key has been handled before comparing.
    session.type_input("zq").unwrap();
    session.wait_for_text("zq", TIMEOUT).unwrap();
    let after = session.screen_contents();
    assert_eq!(
        after.matches("parent task identity").count(),
        before.matches("parent task identity").count(),
        "alt + up does not recall the last prompt:\n{after}"
    );
}

#[test]
fn a_session_that_starts_with_a_delegation_is_titled_by_its_task() {
    let mut session = spawn_fresh(40, 130);
    session
        .submit_input("/delegate mock-model inspect the parser")
        .unwrap();
    session
        .wait_for_text("result from mock-model", TIMEOUT)
        .unwrap();
    session
        .wait_for_text("Delegation continuation received", TIMEOUT)
        .expect("the parent answers the delivered result");
    session.settle(Duration::from_millis(300));
    session.submit_input("/sessions").unwrap();
    session
        .wait_for(|screen| screen.contains("└─ "), TIMEOUT)
        .expect("the child is listed under its parent");
    session.settle(Duration::from_millis(200));
    let screen = session.screen_contents();
    let rows = screen
        .lines()
        .skip_while(|line| !line.contains("Sessions 2"))
        .collect::<Vec<_>>();
    let parent = rows
        .iter()
        .find(|line| line.contains("(current)"))
        .unwrap_or_else(|| panic!("parent row missing:\n{screen}"));
    assert!(parent.contains("inspect the parser"), "{screen}");
    assert!(
        rows.iter()
            .all(|row| !row.contains("[Nori delegation result]")),
        "{screen}"
    );
    let child = rows
        .iter()
        .find(|line| line.contains("└─ "))
        .unwrap_or_else(|| panic!("child row missing:\n{screen}"));
    assert!(
        child.contains("mock-model "),
        "agents are named by slug:\n{screen}"
    );
    assert!(
        child.contains(" done"),
        "a child that finished its task is done:\n{screen}"
    );
    assert!(rows.iter().all(|row| !row.contains("closed")), "{screen}");
}

#[test]
fn the_child_breadcrumb_fits_one_row_of_an_80_column_terminal() {
    let mut session = spawn_fresh(30, 80);
    session
        .submit_input("parent task identity with a long title for the breadcrumb")
        .unwrap();
    session
        .wait_for_text("parent final answer", TIMEOUT)
        .unwrap();
    start_waiting_child(&mut session);
    session.send_key(Key::AltDown).unwrap();
    session
        .wait_for_text("Would you like to allow", TIMEOUT)
        .unwrap();
    session
        .wait_for_text("to parent", TIMEOUT)
        .expect("the child shows the way back to its parent");
    session.settle(Duration::from_millis(200));
    let screen = session.screen_contents();
    let breadcrumb = screen
        .lines()
        .find(|line| line.contains("to parent"))
        .unwrap_or_else(|| panic!("breadcrumb missing:\n{screen}"));
    assert!(
        breadcrumb.contains("mock-model · parent task")
            && breadcrumb.contains("› mock-model-alt · mock:req"),
        "the breadcrumb is one row:\n{screen}"
    );
    assert!(
        breadcrumb.trim_end().ends_with("alt + up to parent"),
        "{screen}"
    );
}

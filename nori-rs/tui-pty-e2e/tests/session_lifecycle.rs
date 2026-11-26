//! Starting, closing and looping sessions beside each other.

use std::time::Duration;
use std::time::Instant;

use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TuiSession;

const ANSWER: &str = "alpha agent answer";
const STREAM: &str = "streaming work mock:stream-until-cancel";

fn config(extra_toml: &str) -> SessionConfig {
    SessionConfig::new()
        .with_extra_config_toml(extra_toml.to_string())
        .with_agent_env("MOCK_AGENT_RESPONSE_MOCK_MODEL", ANSWER)
        .with_agent_env("MOCK_AGENT_RESPONSE_MOCK_MODEL_ALT", "beta agent answer")
}

fn spawn_with(config: SessionConfig) -> TuiSession {
    let mut session = TuiSession::spawn_with_config(40, 110, config).expect("spawn");
    session
        .wait_for_composer(TIMEOUT)
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
        .wait_for_composer(TIMEOUT)
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

/// An open session list keeps its "Ns ago" labels current while nothing else
/// changes, redrawing once a second rather than continuously.
#[test]
#[cfg(target_os = "linux")]
fn an_idle_open_session_list_ticks_its_ages_without_redrawing_continuously() {
    let mut session = spawn_with(config("[tui]\nmax_sessions = 2\n"));
    ask(&mut session, "first question", ANSWER);
    start_new_session(&mut session, "2 ready");
    ask(&mut session, "second question", ANSWER);
    open_list(&mut session, "Sessions 2");
    session.settle(Duration::from_millis(300));

    let before = session.raw_output().len();
    session
        .wait_for(
            |screen| (3..=9).any(|age| screen.contains(&format!("{age}s ago"))),
            Duration::from_secs(8),
        )
        .expect("the ages advance while the list stays open");
    let start = std::time::Instant::now();
    let written_before = session.raw_output().len();
    session.settle(Duration::from_secs(2));
    let written = session.raw_output().len() - written_before;

    assert!(session.raw_output().len() > before);
    assert!(
        written < 16 * 1024,
        "an idle session list wrote {written} bytes in {:?}",
        start.elapsed()
    );
}

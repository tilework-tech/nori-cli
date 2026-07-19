//! Quit/exit ergonomics: quitting must give immediate feedback, stop
//! accepting input, and hard-exit within a short deadline even when the
//! backend teardown stalls. On a cloud agent, quitting is a *detach* — the
//! session keeps running server-side (`session/close` is the only terminal
//! verb), so the feedback must say so.

use super::*;

fn cloud_capabilities() -> nori_protocol::SessionCapabilitiesView {
    nori_protocol::SessionCapabilitiesView {
        agent: nori_protocol::AgentCapabilitiesView {
            http_mcp: false,
            load_session: false,
            session_list: true,
            session_resume: true,
            session_close: true,
        },
        ..Default::default()
    }
}

/// Collect every InsertHistoryCell currently queued and render it to one
/// string for containment asserts.
fn drain_history_text(rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>) -> String {
    drain_insert_history(rx)
        .iter()
        .map(|cell| lines_to_single_string(cell))
        .collect::<Vec<_>>()
        .join("\n")
}

/// /quit must (a) show exiting feedback in the composer immediately, (b) still
/// request the graceful backend shutdown, and (c) force an exit within the
/// hard deadline even if the backend never reports ShutdownComplete — the
/// 25s child-exit grace must never hold the user hostage.
#[tokio::test]
async fn quit_shows_exiting_feedback_and_forces_exit_within_deadline() {
    let (mut chat, mut rx, mut op_rx) = make_chatwidget_manual();

    chat.dispatch_command(SlashCommand::Quit);

    assert_matches!(op_rx.try_recv(), Ok(Op::Shutdown));
    let rendered = render_bottom_popup(&chat, 80);
    assert_eq!(rendered, "› Exiting…");

    // Local exit feedback belongs only in the composer. This drain discards
    // any non-history event queued so far; the watchdog assert below tolerates
    // that because a synchronous ExitRequest at dispatch time would be wrong.
    let feedback = drain_history_text(&mut rx);
    assert!(
        feedback.is_empty(),
        "local quit must not add an exit history cell, got: {feedback:?}"
    );

    // No ShutdownComplete is ever delivered here — the watchdog alone must
    // force the exit. The bound is real time (loaded-CI slack included) and
    // exists to kill the old 25s teardown hostage-taking; do not pause time.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        let event = tokio::time::timeout(remaining, rx.recv())
            .await
            .expect("quit must force ExitRequest within the hard deadline")
            .expect("channel closed");
        if matches!(event, AppEvent::ExitRequest) {
            break;
        }
    }
}

#[test]
fn exiting_composer_hides_draft_popup_footer_and_cursor_snapshot() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    chat.bottom_pane
        .set_composer_text("/model hidden draft".to_string());

    chat.begin_exit();

    let mut terminal = Terminal::new(TestBackend::new(80, 8)).expect("create terminal");
    terminal
        .draw(|f| chat.render(f.area(), f.buffer_mut()))
        .expect("draw exiting composer");
    insta::assert_snapshot!("exiting_composer", terminal.backend());
    assert_eq!(chat.bottom_pane.composer_text(), "/model hidden draft");
    assert_eq!(chat.cursor_pos(Rect::new(0, 0, 80, 8)), None);
}

#[test]
fn exit_discards_buffered_paste_burst() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    chat.bottom_pane
        .set_composer_text("enabled control".to_string());
    chat.bottom_pane
        .handle_key_event(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
    std::thread::sleep(std::time::Duration::from_millis(20));
    chat.bottom_pane.flush_paste_burst_if_due();
    assert_eq!(chat.bottom_pane.composer_text(), "xenabled control");

    chat.bottom_pane
        .set_composer_text("preserved draft".to_string());
    chat.bottom_pane
        .handle_key_event(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
    chat.begin_exit();
    std::thread::sleep(std::time::Duration::from_millis(20));
    chat.bottom_pane.flush_paste_burst_if_due();

    assert_eq!(chat.bottom_pane.composer_text(), "preserved draft");
}

#[test]
fn exiting_composer_refuses_interactive_mutations() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    chat.bottom_pane
        .attach_image(PathBuf::from("/tmp/enabled.png"), 10, 5, "PNG");
    assert_eq!(
        chat.bottom_pane.take_recent_submission_images(),
        vec![PathBuf::from("/tmp/enabled.png")]
    );
    chat.bottom_pane
        .set_composer_text("preserved draft".to_string());

    chat.begin_exit();

    assert_eq!(
        chat.bottom_pane
            .handle_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
        InputResult::None
    );
    assert_eq!(
        chat.bottom_pane
            .handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        InputResult::None
    );
    chat.bottom_pane
        .handle_key_event(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE));
    chat.bottom_pane.handle_paste(" pasted".to_string());
    chat.bottom_pane.insert_str(" inserted");
    chat.bottom_pane
        .attach_image(PathBuf::from("/tmp/blocked.png"), 10, 5, "PNG");

    assert_eq!(chat.bottom_pane.composer_text(), "preserved draft");
    assert!(chat.bottom_pane.take_recent_submission_images().is_empty());
    std::thread::sleep(std::time::Duration::from_millis(20));
    chat.bottom_pane.flush_paste_burst_if_due();
    assert_eq!(chat.bottom_pane.composer_text(), "preserved draft");
}

#[test]
fn exiting_closes_active_bottom_pane_view() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    chat.show_acp_resume_session_picker(vec![nori_harness::AcpSessionSummary {
        session_id: "session-1".to_string(),
        cwd: PathBuf::from("/tmp"),
        title: Some("Session one".to_string()),
        updated_at: None,
        meta: None,
    }]);
    assert!(chat.bottom_pane.has_active_view());

    chat.begin_exit();

    assert!(!chat.bottom_pane.has_active_view());
    assert_eq!(render_bottom_popup(&chat, 80), "› Exiting…");
}

#[test]
fn exiting_refuses_task_status_escape_interrupt() {
    let (mut chat, mut rx, mut op_rx) = make_chatwidget_manual();
    chat.bottom_pane.set_task_running(true);
    chat.begin_exit();
    assert_matches!(op_rx.try_recv(), Ok(Op::Shutdown));
    while rx.try_recv().is_ok() {}

    assert_eq!(
        chat.bottom_pane
            .handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        InputResult::None
    );

    assert!(
        !matches!(rx.try_recv(), Ok(AppEvent::CodexOp(Op::Interrupt))),
        "Esc must not interrupt a task after exit has disabled input"
    );
}

#[test]
fn repeated_exit_requests_submit_shutdown_once() {
    let (mut chat, _rx, mut op_rx) = make_chatwidget_manual();

    chat.begin_exit();
    chat.begin_exit();

    assert_matches!(op_rx.try_recv(), Ok(Op::Shutdown));
    assert!(op_rx.try_recv().is_err());
}

/// After quit is requested the composer is done: submitting a prompt must not
/// reach the backend, and session-switching slash commands must not fire. A
/// fast typist could otherwise start a whole new turn during teardown.
#[tokio::test]
async fn exiting_blocks_prompt_submission_and_session_commands() {
    let (mut chat, mut rx, mut op_rx) = make_chatwidget_manual();

    chat.dispatch_command(SlashCommand::Quit);
    while op_rx.try_recv().is_ok() {}
    while rx.try_recv().is_ok() {}

    chat.submit_user_message("sneaky final prompt".to_string().into());
    assert!(
        op_rx.try_recv().is_err(),
        "a prompt submitted while exiting must not reach the backend"
    );

    chat.dispatch_command(SlashCommand::New);
    let mut saw_new_session = false;
    while let Ok(event) = rx.try_recv() {
        if matches!(event, AppEvent::NewSession) {
            saw_new_session = true;
        }
    }
    assert!(
        !saw_new_session,
        "/new must not start a session while the app is exiting"
    );
}

/// A prompt submitted with no live backend (deferred spawn before a picker
/// choice, or after the backend shut down) must explain itself — the old
/// behavior echoed the prompt into history and silently dropped it.
#[tokio::test]
async fn prompts_without_a_live_agent_explain_instead_of_vanishing() {
    let (mut chat, mut rx, op_rx) = make_chatwidget_manual();
    // Dropping the op receiver reproduces the deferred widget's dummy
    // channel: sends have nowhere to go.
    drop(op_rx);

    chat.submit_user_message("hello nobody".to_string().into());

    let feedback = drain_history_text(&mut rx);
    assert!(
        feedback.contains("No active session"),
        "a dead-channel prompt must produce an explanation, got: {feedback:?}"
    );
    assert!(
        !feedback.contains("hello nobody"),
        "the prompt must not be echoed as if it were sent, got: {feedback:?}"
    );
}

/// Idle Ctrl+C is a quit entry point too — it must show the same composer
/// feedback instead of leaving the user staring at a frozen prompt.
#[tokio::test]
async fn ctrl_c_when_idle_begins_exit_with_feedback() {
    let (mut chat, mut rx, mut op_rx) = make_chatwidget_manual();

    chat.on_ctrl_c();

    assert_matches!(op_rx.try_recv(), Ok(Op::Shutdown));
    assert_eq!(render_bottom_popup(&chat, 80), "› Exiting…");
    assert!(drain_history_text(&mut rx).is_empty());
}

/// Snapshot: the agent-sourced session picker as it appears on `nori cloud`
/// entry — create-new row first, then live sessions by broker title.
#[test]
fn acp_session_picker_with_create_new_row_snapshot() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    chat.show_acp_resume_session_picker(vec![
        nori_harness::AcpSessionSummary {
            session_id: "nori-dense-katsu-cd8a".to_string(),
            cwd: std::path::PathBuf::from("/"),
            title: Some("Fix the flaky login test".to_string()),
            updated_at: None,
            meta: None,
        },
        nori_harness::AcpSessionSummary {
            session_id: "nori-trim-anago-ff79".to_string(),
            cwd: std::path::PathBuf::from("/"),
            title: None,
            updated_at: None,
            meta: None,
        },
    ]);

    let popup = render_bottom_popup(&chat, 80);
    insta::assert_snapshot!("acp_session_picker_with_create_new", popup);
}

/// Snapshot: the cloud-only detach note shown in history when quit begins.
#[tokio::test]
async fn cloud_quit_detach_feedback_snapshot() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.handle_client_event(nori_protocol::ClientEvent::SessionCapabilitiesChanged(
        cloud_capabilities(),
    ));

    chat.dispatch_command(SlashCommand::Quit);

    let cells = drain_insert_history(&mut rx);
    let rendered = cells
        .iter()
        .map(|cell| lines_to_single_string(cell))
        .collect::<Vec<_>>()
        .join("");
    insta::assert_snapshot!("cloud_quit_detach_feedback", rendered);
}

/// On a cloud agent (live-reattach capable, close-capable), quitting detaches:
/// the session keeps running server-side. The exiting feedback must say so —
/// otherwise users will assume quit killed their session.
#[tokio::test]
async fn quit_on_cloud_agent_explains_the_detach() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.handle_client_event(nori_protocol::ClientEvent::SessionCapabilitiesChanged(
        cloud_capabilities(),
    ));

    chat.dispatch_command(SlashCommand::Quit);

    assert_eq!(render_bottom_popup(&chat, 80), "› Exiting…");
    let feedback = drain_history_text(&mut rx);
    assert!(
        feedback.contains("keeps running"),
        "cloud quit feedback must explain the session keeps running (detach), got: {feedback:?}"
    );
}

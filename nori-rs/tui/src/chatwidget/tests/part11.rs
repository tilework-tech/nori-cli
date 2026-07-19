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

/// /quit must replace the composer with immediate exit feedback and still
/// force an exit if backend teardown stalls.
#[tokio::test]
async fn quit_shows_exit_composer_and_forces_exit_within_deadline() {
    let (mut chat, mut rx, mut op_rx) = make_chatwidget_manual();
    chat.insert_str("draft that must be preserved");

    chat.dispatch_command(SlashCommand::Quit);

    assert_matches!(op_rx.try_recv(), Ok(Op::Shutdown));
    assert_eq!(drain_history_text(&mut rx), "");
    let rendered = render_bottom_popup(&chat, 80);
    assert!(rendered.contains("› Exiting…"), "got: {rendered:?}");
    assert!(!rendered.contains("draft that must be preserved"));
    assert!(!rendered.contains("Ask Nori to do anything"));
    let area = Rect::new(0, 0, 80, chat.desired_height(80));
    assert_eq!(chat.cursor_pos(area), None);
    assert_eq!(
        chat.bottom_pane.composer_text(),
        "draft that must be preserved"
    );
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

/// Once exit starts, all composer mutation paths are inert and transient
/// popups or paste bursts are cleared.
#[tokio::test]
async fn exiting_blocks_all_composer_mutation_paths() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    chat.insert_str("/new");
    assert!(chat.bottom_pane.has_active_overlay_or_popup());
    chat.bottom_pane
        .handle_key_event(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));

    chat.dispatch_command(SlashCommand::Quit);
    chat.handle_key_event(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
    chat.handle_paste(" pasted".to_string());
    chat.attach_image(PathBuf::from("ignored.png"), 10, 20, "PNG");
    std::thread::sleep(crate::bottom_pane::ChatComposer::recommended_paste_flush_delay());

    assert!(!chat.bottom_pane.flush_paste_burst_if_due());
    assert_eq!(chat.bottom_pane.composer_text(), "/new");
    assert!(!chat.bottom_pane.has_active_overlay_or_popup());
}

#[tokio::test]
async fn exiting_closes_bottom_pane_views() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    chat.show_acp_resume_session_picker(Vec::new());
    assert!(chat.bottom_pane.has_active_view());

    chat.dispatch_command(SlashCommand::Quit);

    assert!(!chat.bottom_pane.has_active_view());
    assert!(render_bottom_popup(&chat, 80).contains("› Exiting…"));
}

#[tokio::test]
async fn repeated_cloud_exit_requests_are_idempotent() {
    let (mut chat, mut rx, mut op_rx) = make_chatwidget_manual();
    chat.handle_client_event(nori_protocol::ClientEvent::SessionCapabilitiesChanged(
        cloud_capabilities(),
    ));

    chat.dispatch_command(SlashCommand::Quit);
    chat.dispatch_command(SlashCommand::Exit);

    assert_matches!(op_rx.try_recv(), Ok(Op::Shutdown));
    assert!(op_rx.try_recv().is_err());
    assert_eq!(
        drain_history_text(&mut rx)
            .matches("This session keeps running in the cloud.")
            .count(),
        1
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

/// Idle Ctrl+C must show the same disabled composer.
#[tokio::test]
async fn ctrl_c_when_idle_shows_exit_composer() {
    let (mut chat, mut rx, mut op_rx) = make_chatwidget_manual();

    chat.on_ctrl_c();

    assert_matches!(op_rx.try_recv(), Ok(Op::Shutdown));
    assert_eq!(drain_history_text(&mut rx), "");
    assert!(render_bottom_popup(&chat, 80).contains("› Exiting…"));
}

#[tokio::test]
async fn exit_composer_snapshot() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    chat.insert_str("hidden draft");
    chat.dispatch_command(SlashCommand::Quit);
    insta::assert_snapshot!("exit_composer", render_bottom_popup(&chat, 80));
}

#[tokio::test]
async fn exit_composer_prompt_and_message_are_dimmed() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    chat.dispatch_command(SlashCommand::Quit);
    let area = Rect::new(0, 0, 80, chat.desired_height(80));
    let mut buf = Buffer::empty(area);
    chat.render(area, &mut buf);

    let prompt = (0..area.height)
        .flat_map(|y| (0..area.width).map(move |x| (x, y)))
        .find(|&(x, y)| buf[(x, y)].symbol() == "›")
        .expect("exit prompt");
    let message = (0..area.height)
        .flat_map(|y| (0..area.width).map(move |x| (x, y)))
        .find(|&(x, y)| buf[(x, y)].symbol() == "E")
        .expect("exit message");

    assert!(buf[prompt].modifier.contains(ratatui::style::Modifier::DIM));
    assert!(
        buf[message]
            .modifier
            .contains(ratatui::style::Modifier::DIM)
    );
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

/// Snapshot: the cloud lifecycle note recorded when detach begins.
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

    let feedback = drain_history_text(&mut rx);
    assert!(feedback.contains("This session keeps running in the cloud."));
    assert!(!feedback.contains("Exiting"));
    assert!(render_bottom_popup(&chat, 80).contains("› Exiting…"));
}

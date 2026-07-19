//! Cloud/local scoping for builtin slash commands. A *cloud* session runs the
//! agent on a remote VM, so local-machine commands (`/switch-skillset`,
//! `/browse`, `/diff`, `/browser`) are meaningless there and must be
//! unavailable — greyed in the popup, and a typed dispatch shows an error cell
//! and does nothing else. `/close` is the inverse: it is cloud-only and must be
//! unavailable on a non-cloud agent (replacing today's bespoke gate). `/quit`
//! and `/exit` are never disabled. Scope reacts to capability changes, which
//! arrive after initialize (SessionCapabilitiesChanged) and change again after
//! a picker resume.

use super::*;

/// Cloud session: a typed local-only command (`/diff`) must be rejected with an
/// "unavailable" error that names the cloud reason, and no history cell beyond
/// the error may appear. (Diff's effect is an async AppEvent, so this pins
/// "no extra unexpected cell", not the absence of the diff spawn itself.)
#[tokio::test]
async fn cloud_session_disables_local_only_command() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.handle_client_event(nori_protocol::ClientEvent::SessionCapabilitiesChanged(
        cloud_capabilities(),
    ));

    chat.dispatch_command(SlashCommand::Diff);

    let cells = drain_insert_history(&mut rx);
    let feedback = cells_to_text(&cells);
    assert!(
        feedback.contains("/diff is unavailable"),
        "cloud session must reject /diff with an unavailable error, got: {feedback:?}"
    );
    assert!(
        feedback.to_lowercase().contains("cloud"),
        "the /diff unavailable reason must explain it is a local-only command on a cloud session, got: {feedback:?}"
    );
    assert_eq!(
        cells.len(),
        1,
        "the blocked /diff must not also run its normal effect (only the error cell), got: {feedback:?}"
    );
}

/// A local (non-cloud) agent must keep local-only commands available: `/diff`
/// must not produce an "unavailable" error. Guards against over-broad disabling.
#[tokio::test]
async fn local_session_keeps_local_only_command_available() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.handle_client_event(nori_protocol::ClientEvent::SessionCapabilitiesChanged(
        local_capabilities(true),
    ));

    chat.dispatch_command(SlashCommand::Diff);

    let feedback = cells_to_text(&drain_insert_history(&mut rx));
    assert!(
        !feedback.contains("unavailable"),
        "a local agent must not mark /diff unavailable, got: {feedback:?}"
    );
}

/// `/close` is cloud-only: on an agent without the `session_close` capability it
/// must be rejected through the unified availability mechanism (the same
/// "/close is unavailable." wording every other scoped command uses), with a
/// reason that mentions session close — and no close must actually be attempted.
#[tokio::test]
async fn close_unavailable_without_session_close_capability() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.handle_client_event(nori_protocol::ClientEvent::SessionCapabilitiesChanged(
        local_capabilities(false),
    ));

    chat.dispatch_command(SlashCommand::Close);

    let mut saw_session_closed = false;
    let mut cells: Vec<Vec<ratatui::text::Line<'static>>> = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        match ev {
            AppEvent::InsertHistoryCell(cell) => cells.push(cell.display_lines(80)),
            AppEvent::SessionClosed => saw_session_closed = true,
            _ => {}
        }
    }
    let feedback = cells_to_text(&cells);
    assert!(
        feedback.contains("/close is unavailable"),
        "/close must be rejected through the unified availability mechanism, got: {feedback:?}"
    );
    assert!(
        feedback.contains("session/close"),
        "the /close unavailable reason must name the session/close capability, got: {feedback:?}"
    );
    assert!(
        !feedback.contains("is disabled for the active session"),
        "the /close reason must be the scoped explanation, not the generic fallback, got: {feedback:?}"
    );
    assert!(
        !saw_session_closed,
        "a blocked /close must not attempt to close the session"
    );
    assert!(
        !chat.session_close_in_flight,
        "a blocked /close must not mark a close in flight"
    );
}

/// Scope reacts to capability changes: the same command is blocked under cloud
/// capabilities, then becomes available again once non-cloud capabilities
/// arrive (e.g. after a picker resume onto a local agent).
#[tokio::test]
async fn scope_reacts_to_capability_changes() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.handle_client_event(nori_protocol::ClientEvent::SessionCapabilitiesChanged(
        cloud_capabilities(),
    ));
    chat.dispatch_command(SlashCommand::Diff);
    let blocked = cells_to_text(&drain_insert_history(&mut rx));
    assert!(
        blocked.contains("/diff is unavailable"),
        "under cloud capabilities /diff must be blocked, got: {blocked:?}"
    );

    chat.handle_client_event(nori_protocol::ClientEvent::SessionCapabilitiesChanged(
        local_capabilities(true),
    ));
    chat.dispatch_command(SlashCommand::Diff);
    let after = cells_to_text(&drain_insert_history(&mut rx));
    assert!(
        !after.contains("unavailable"),
        "after non-cloud capabilities arrive /diff must be available again, got: {after:?}"
    );
}

/// `/quit` must never be disabled by scope — even on a cloud session it must
/// still begin the exit and show the immediate exiting feedback.
#[tokio::test]
async fn quit_never_disabled_on_cloud_session() {
    let (mut chat, mut rx, mut op_rx) = make_chatwidget_manual();
    chat.handle_client_event(nori_protocol::ClientEvent::SessionCapabilitiesChanged(
        cloud_capabilities(),
    ));

    chat.dispatch_command(SlashCommand::Quit);

    assert_matches!(op_rx.try_recv(), Ok(Op::Shutdown));
    let feedback = cells_to_text(&drain_insert_history(&mut rx));
    assert!(feedback.contains("This session keeps running in the cloud."));
    assert!(!feedback.contains("unavailable"));
    assert!(render_bottom_popup(&chat, 80).contains("› Exiting…"));
}

/// Snapshot: with cloud capabilities the slash popup rows for local-only
/// commands (here `/browse` and `/browser`, which both match a "/browse"
/// filter) must show the unavailable *reason* in place of the normal
/// description. The render helper captures plain symbols, so this pins the
/// reason-text substitution, not the dim styling. Exercises the real pipeline:
/// SessionCapabilitiesChanged updates the popup state, then opening the popup
/// renders the disabled rows. (`insert_str` inserts directly and syncs the
/// popup, avoiding the paste-burst heuristic that buffers char-by-char key
/// input in tests.)
#[test]
fn cloud_slash_popup_greys_local_only_commands_snapshot() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    chat.handle_client_event(nori_protocol::ClientEvent::SessionCapabilitiesChanged(
        cloud_capabilities(),
    ));

    chat.insert_str("/browse");

    let popup = render_bottom_popup(&chat, 80);
    insta::assert_snapshot!("cloud_slash_popup_greys_local_only_commands", popup);
}

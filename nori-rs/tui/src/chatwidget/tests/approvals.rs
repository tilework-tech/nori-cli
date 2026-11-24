//! `/approvals` picks the host mode of the session on screen. The footer
//! shows the mode the host announces for that session, and a choice never
//! changes the settings later sessions start with.

use super::*;
use crate::host_client::HostCall;
use nori_protocol::ApprovalMode;
use pretty_assertions::assert_eq;

fn render_bottom_pane(chat: &ChatWidget) -> String {
    use crate::render::renderable::Renderable;
    let area = ratatui::layout::Rect::new(0, 0, 100, chat.bottom_pane.desired_height(100));
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    chat.bottom_pane.render(area, &mut buffer);
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn announce(chat: &mut ChatWidget, mode: ApprovalMode) {
    let generation = chat.session_generation;
    chat.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::ApprovalModeChanged(mode)),
    );
}

fn host_session(chat: &mut ChatWidget) -> String {
    let id = nori_harness::ConversationId::new();
    chat.managed_conversation_id = Some(id);
    id.to_string()
}

fn press(chat: &mut ChatWidget, code: KeyCode) {
    chat.handle_key_event(KeyEvent::new(code, KeyModifiers::NONE));
}

/// Opens `/approvals`, moves to the row `rows_down` below the first, and
/// presses Enter. Returns the host calls the widget asked for.
fn choose(
    chat: &mut ChatWidget,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    rows_down: i32,
) -> Vec<HostCall> {
    chat.dispatch_command(crate::slash_command::SlashCommand::Approvals);
    press(chat, KeyCode::Home);
    for _ in 0..rows_down {
        press(chat, KeyCode::Down);
    }
    press(chat, KeyCode::Enter);
    host_calls(chat, rx)
}

/// Every host call the widget asked for since the last look, opening the
/// confirmation the App would open on the way.
fn host_calls(
    chat: &mut ChatWidget,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) -> Vec<HostCall> {
    let mut calls = Vec::new();
    while let Ok(event) = rx.try_recv() {
        match event {
            AppEvent::HostRequest { call, .. } => calls.push(call),
            AppEvent::OpenApproveAllConfirmation { session_id } => {
                chat.open_approve_all_confirmation(session_id);
            }
            _ => {}
        }
    }
    calls
}

fn mode_change(call: &HostCall) -> Option<(String, ApprovalMode)> {
    match call {
        HostCall::SetMode { session_id, mode } => Some((session_id.clone(), *mode)),
        _ => None,
    }
}

#[tokio::test]
async fn the_menu_offers_the_host_modes_with_the_session_mode_current() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    host_session(&mut chat);
    announce(&mut chat, ApprovalMode::Ask);

    chat.dispatch_command(crate::slash_command::SlashCommand::Approvals);

    insta::assert_snapshot!("host_mode_menu", render_bottom_pane(&chat));
}

#[tokio::test]
async fn choosing_a_mode_changes_only_the_session_on_screen() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let session = host_session(&mut chat);
    announce(&mut chat, ApprovalMode::Agent);
    let starting_policy = chat.config.approval_policy;

    let calls = choose(&mut chat, &mut rx, 0);

    assert_eq!(
        calls.iter().filter_map(mode_change).collect::<Vec<_>>(),
        vec![(session, ApprovalMode::Ask)]
    );
    assert_eq!(chat.config.approval_policy, starting_policy);
}

#[tokio::test]
async fn approve_all_asks_before_the_host_is_told() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let session = host_session(&mut chat);
    announce(&mut chat, ApprovalMode::Agent);

    let before_confirming = choose(&mut chat, &mut rx, 2);
    let confirmation = render_bottom_pane(&chat);
    press(&mut chat, KeyCode::Enter);
    let after_confirming: Vec<_> = host_calls(&mut chat, &mut rx)
        .iter()
        .filter_map(mode_change)
        .collect();

    assert_eq!(before_confirming.iter().filter_map(mode_change).count(), 0);
    insta::assert_snapshot!("approve_all_confirmation", confirmation);
    assert_eq!(after_confirming, vec![(session, ApprovalMode::ApproveAll)]);
}

#[tokio::test]
async fn the_footer_follows_the_mode_the_host_announces() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    host_session(&mut chat);

    announce(&mut chat, ApprovalMode::ApproveAll);
    let approving = render_bottom_pane(&chat);
    announce(&mut chat, ApprovalMode::Ask);
    let asking = render_bottom_pane(&chat);

    assert!(approving.contains("Approvals: Approve all"), "{approving}");
    assert!(asking.contains("Approvals: Ask"), "{asking}");
}

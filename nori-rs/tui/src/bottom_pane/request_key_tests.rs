//! How the permission prompt reads keys: option keys wait out a short grace
//! period after a request appears, and Esc rejects.

use super::*;
use crate::answer_guard::ManualClock;
use crate::answer_guard::REQUEST_ANSWER_GRACE;
use crate::app_event::AppEvent;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use nori_protocol::acp::v1 as acp;
use pretty_assertions::assert_eq;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::mpsc::unbounded_channel;

fn request(id: &str) -> ApprovalRequest {
    ApprovalRequest {
        request_id: id.to_string().into(),
        title: "Run echo ok".to_string(),
        kind: crate::presentation::ToolKind::Execute,
        cwd: std::env::current_dir().expect("current directory"),
        snapshot: Box::new(crate::presentation::ToolSnapshot {
            call_id: format!("call-{id}"),
            title: "Run echo ok".to_string(),
            kind: crate::presentation::ToolKind::Execute,
            phase: crate::presentation::ToolPhase::PendingApproval,
            locations: Vec::new(),
            invocation: Some(crate::presentation::Invocation::Command {
                command: "echo ok".to_string(),
            }),
            artifacts: Vec::new(),
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        }),
        options: vec![
            acp::PermissionOption::new(
                acp::PermissionOptionId::new("allow-once"),
                "Allow",
                acp::PermissionOptionKind::AllowOnce,
            ),
            acp::PermissionOption::new(
                acp::PermissionOptionId::new("reject-once"),
                "Reject",
                acp::PermissionOptionKind::RejectOnce,
            ),
        ],
    }
}

fn allow_only(id: &str) -> ApprovalRequest {
    let mut request = request(id);
    request.options.truncate(1);
    request
}

fn pane(clock: &ManualClock) -> (BottomPane, UnboundedReceiver<AppEvent>) {
    let (tx, rx) = unbounded_channel::<AppEvent>();
    let pane = BottomPane::new(BottomPaneParams {
        app_event_tx: AppEventSender::new(tx),
        frame_requester: FrameRequester::test_dummy(),
        has_input_focus: true,
        enhanced_keys_supported: false,
        placeholder_text: "Ask Nori to do anything".to_string(),
        disable_paste_burst: true,
        animations_enabled: false,
        custom_working_messages: false,
        custom_working_message_list: Vec::new(),
        vertical_footer: false,
        footer_segment_config: nori_config::FooterSegmentConfig::default(),
        footer_layout_config: nori_config::FooterLayoutConfig::default(),
        agent_display_name: String::new(),
        agent_slug: String::new(),
        clock: clock.clock(),
    });
    (pane, rx)
}

/// Every answer sent to the agent so far, as `(request id, outcome)`; the
/// outcome is the chosen option id or `cancelled`.
fn answers(rx: &mut UnboundedReceiver<AppEvent>) -> Vec<(String, String)> {
    let mut answers = Vec::new();
    while let Ok(event) = rx.try_recv() {
        if let AppEvent::HarnessAction(crate::app_event::HarnessAction::RespondToAgent {
            request_id,
            response,
        }) = event
            && let Ok(acp::ClientResponse::RequestPermissionResponse(response)) = *response
        {
            let outcome = match response.outcome {
                acp::RequestPermissionOutcome::Selected(selected) => selected.option_id.to_string(),
                acp::RequestPermissionOutcome::Cancelled => "cancelled".to_string(),
                _ => "other".to_string(),
            };
            answers.push((request_id.to_string(), outcome));
        }
    }
    answers
}

fn press(pane: &mut BottomPane, code: KeyCode) {
    pane.handle_key_event(KeyEvent::new(code, KeyModifiers::NONE));
}

#[test]
fn option_keys_typed_as_a_request_appears_do_not_answer_it() {
    for code in [KeyCode::Char('1'), KeyCode::Enter, KeyCode::Char('y')] {
        let clock = ManualClock::new();
        let (mut pane, mut rx) = pane(&clock);
        pane.insert_str("my draft");
        pane.push_approval_request(request("1"));
        clock.advance(REQUEST_ANSWER_GRACE - std::time::Duration::from_millis(1));

        press(&mut pane, code);

        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(format!("{event:?}"));
        }
        assert_eq!(
            events,
            Vec::<String>::new(),
            "{code:?} reached past the prompt"
        );
        assert!(pane.has_active_view(), "{code:?} closed the prompt");
        assert_eq!(
            pane.composer_text(),
            "my draft",
            "{code:?} changed the draft"
        );
    }
}

#[test]
fn option_keys_answer_once_the_request_has_been_visible_for_the_grace_period() {
    for (code, option) in [
        (KeyCode::Char('1'), "allow-once"),
        (KeyCode::Enter, "allow-once"),
        (KeyCode::Char('y'), "allow-once"),
        (KeyCode::Char('n'), "reject-once"),
        (KeyCode::Char('2'), "reject-once"),
    ] {
        let clock = ManualClock::new();
        let (mut pane, mut rx) = pane(&clock);
        pane.push_approval_request(request("1"));
        clock.pass_grace();

        press(&mut pane, code);

        assert_eq!(
            answers(&mut rx),
            vec![("1".to_string(), option.to_string())],
            "{code:?}"
        );
    }
}

#[test]
fn the_next_queued_request_gets_its_own_grace_period() {
    let clock = ManualClock::new();
    let (mut pane, mut rx) = pane(&clock);
    pane.push_approval_request(request("1"));
    pane.push_approval_request(request("2"));
    clock.pass_grace();
    press(&mut pane, KeyCode::Char('1'));
    let first = answers(&mut rx);
    assert_eq!(first.len(), 1, "one request is answered: {first:?}");

    press(&mut pane, KeyCode::Char('1'));
    assert_eq!(
        answers(&mut rx),
        Vec::new(),
        "the next request just appeared"
    );

    clock.pass_grace();
    press(&mut pane, KeyCode::Char('1'));
    let second = answers(&mut rx);
    assert_eq!(second.len(), 1, "the next request is answered: {second:?}");
    assert_ne!(first[0].0, second[0].0, "each request is answered once");
}

#[test]
fn esc_rejects_the_request_even_during_the_grace_period() {
    let clock = ManualClock::new();
    let (mut pane, mut rx) = pane(&clock);
    pane.push_approval_request(request("1"));

    press(&mut pane, KeyCode::Esc);

    assert_eq!(
        answers(&mut rx),
        vec![("1".to_string(), "reject-once".to_string())]
    );
    assert!(!pane.has_active_view(), "the prompt closes");
}

#[test]
fn esc_cancels_a_request_that_offers_no_reject_option() {
    let clock = ManualClock::new();
    let (mut pane, mut rx) = pane(&clock);
    pane.push_approval_request(allow_only("1"));

    press(&mut pane, KeyCode::Esc);

    assert_eq!(
        answers(&mut rx),
        vec![("1".to_string(), "cancelled".to_string())]
    );
}

#[test]
fn ctrl_c_cancels_the_request_even_during_the_grace_period() {
    let clock = ManualClock::new();
    let (mut pane, mut rx) = pane(&clock);
    pane.push_approval_request(request("1"));

    pane.on_ctrl_c();

    assert_eq!(
        answers(&mut rx),
        vec![("1".to_string(), "cancelled".to_string())]
    );
}

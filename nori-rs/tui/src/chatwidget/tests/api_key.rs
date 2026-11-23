use super::*;
use crate::chatwidget::api_key::ApiKeyPreference;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;

fn render_bottom_pane(chat: &ChatWidget) -> String {
    use crate::render::renderable::Renderable;
    let area = ratatui::layout::Rect::new(0, 0, 80, chat.bottom_pane.desired_height(80));
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

fn claude(prefer_api_key: bool, api_billing_confirmed: bool) -> ApiKeyPreference {
    ApiKeyPreference {
        agent: "claude-code".to_string(),
        display_name: "Claude Code".to_string(),
        prefer_api_key,
        api_billing_confirmed,
        saved_providers: vec!["anthropic".to_string()],
        billing_warning: Some(
            "Claude Code is signed in with a subscription (max). Preferring the saved API key \
             bills usage to that API account, not the subscription."
                .to_string(),
        ),
    }
}

fn press(chat: &mut ChatWidget, code: KeyCode) {
    chat.handle_key_event(KeyEvent::new(code, KeyModifiers::NONE));
}

#[derive(Debug, PartialEq)]
enum Sent {
    Confirm(ApiKeyPreference),
    Persist {
        agent: String,
        prefer_api_key: bool,
        api_billing_confirmed: bool,
    },
}

fn sent(rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>) -> Vec<Sent> {
    let mut out = Vec::new();
    while let Ok(event) = rx.try_recv() {
        match event {
            AppEvent::OpenApiKeyBillingConfirmation(preference) => {
                out.push(Sent::Confirm(preference));
            }
            AppEvent::PersistApiKeyPreference {
                agent,
                prefer_api_key,
                api_billing_confirmed,
            } => out.push(Sent::Persist {
                agent,
                prefer_api_key,
                api_billing_confirmed,
            }),
            _ => {}
        }
    }
    out
}

#[test]
fn api_key_picker_offers_the_agents_own_login_and_the_saved_key() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    chat.open_api_key_picker(claude(false, false));
    insta::assert_snapshot!("api_key_picker", render_bottom_pane(&chat));
}

#[test]
fn preferring_the_key_over_a_subscription_warns_before_saving() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.open_api_key_picker(claude(false, false));
    press(&mut chat, KeyCode::Down);
    press(&mut chat, KeyCode::Enter);
    let asked = sent(&mut rx);
    assert_eq!(asked, vec![Sent::Confirm(claude(false, false))]);

    chat.open_api_key_billing_confirmation(claude(false, false));
    insta::assert_snapshot!("api_key_billing_confirmation", render_bottom_pane(&chat));
    press(&mut chat, KeyCode::Up);
    press(&mut chat, KeyCode::Enter);

    assert_eq!(
        sent(&mut rx),
        vec![Sent::Persist {
            agent: "claude-code".to_string(),
            prefer_api_key: true,
            api_billing_confirmed: true,
        }]
    );
}

#[test]
fn enter_or_escape_on_the_billing_warning_saves_nothing() {
    for key in [KeyCode::Enter, KeyCode::Esc] {
        let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
        chat.open_api_key_billing_confirmation(claude(false, false));
        press(&mut chat, key);
        assert_eq!(sent(&mut rx), vec![], "{key:?}");
    }
}

#[test]
fn a_confirmed_warning_is_not_asked_again() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.open_api_key_picker(claude(false, true));
    press(&mut chat, KeyCode::Down);
    press(&mut chat, KeyCode::Enter);
    assert_eq!(
        sent(&mut rx),
        vec![Sent::Persist {
            agent: "claude-code".to_string(),
            prefer_api_key: true,
            api_billing_confirmed: true,
        }]
    );
}

#[test]
fn going_back_to_the_agents_login_keeps_the_remembered_answer() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.open_api_key_picker(claude(true, true));
    press(&mut chat, KeyCode::Up);
    press(&mut chat, KeyCode::Enter);
    assert_eq!(
        sent(&mut rx),
        vec![Sent::Persist {
            agent: "claude-code".to_string(),
            prefer_api_key: false,
            api_billing_confirmed: true,
        }]
    );
}

#[test]
fn without_a_subscription_the_key_is_preferred_without_a_warning() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.open_api_key_picker(ApiKeyPreference {
        billing_warning: None,
        ..claude(false, false)
    });
    press(&mut chat, KeyCode::Down);
    press(&mut chat, KeyCode::Enter);
    assert_eq!(
        sent(&mut rx),
        vec![Sent::Persist {
            agent: "claude-code".to_string(),
            prefer_api_key: true,
            api_billing_confirmed: false,
        }]
    );
}

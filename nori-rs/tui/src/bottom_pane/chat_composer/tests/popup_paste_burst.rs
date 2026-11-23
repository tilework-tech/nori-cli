use super::*;
use crate::slash_command::SlashCommand;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;

#[test]
fn enter_in_slash_popup_dispatches_the_full_command_after_a_held_fast_tail() {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );

    // A prefix that matches several commands, followed by the rest of the
    // name arriving too fast to render before Enter is processed.
    type_chars_humanlike(&mut composer, &['/', 'c', 'o']);
    for ch in "ntext".chars() {
        let _ = composer.handle_key_event(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    std::thread::sleep(ChatComposer::recommended_paste_flush_delay());

    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(result, InputResult::Command(SlashCommand::Context));
}

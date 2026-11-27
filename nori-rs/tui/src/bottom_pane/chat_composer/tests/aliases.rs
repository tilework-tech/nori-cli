//! `/agents` names the `/agent` builtin in plain `nori` and in `nori attach`,
//! and the slash menu still lists that builtin once.

use super::*;
use crate::render::renderable::Renderable;
use crate::slash_command::SlashCommand;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;

fn plain() -> ChatComposer {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    ChatComposer::new(
        true,
        AppEventSender::new(tx),
        false,
        "Ask Nori to do anything".to_string(),
        false,
    )
}

fn joined() -> ChatComposer {
    let mut composer = plain();
    composer.set_host_status(Some(Line::from("controlling")));
    composer
}

fn type_text(composer: &mut ChatComposer, text: &str) {
    type_chars_humanlike(composer, &text.chars().collect::<Vec<_>>());
}

fn enter(composer: &mut ChatComposer) -> InputResult {
    composer
        .handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .0
}

fn screen(composer: &ChatComposer) -> String {
    let area = Rect::new(0, 0, 100, 30);
    let mut buffer = Buffer::empty(area);
    composer.render(area, &mut buffer);
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn agents_runs_the_agent_command_in_plain_nori_and_in_attach() {
    for (surface, mut composer) in [("plain", plain()), ("joined", joined())] {
        type_text(&mut composer, "/agents");
        assert_eq!(
            enter(&mut composer),
            InputResult::Command(SlashCommand::Agent),
            "/agents in the {surface} terminal"
        );
        assert_eq!(composer.current_text(), "", "{surface} composer is cleared");
    }
}

#[test]
fn agents_after_tab_completion_still_runs_the_agent_command() {
    for (surface, mut composer) in [("plain", plain()), ("joined", joined())] {
        type_text(&mut composer, "/agents ");
        assert_eq!(
            enter(&mut composer),
            InputResult::Command(SlashCommand::Agent),
            "/agents with a trailing space in the {surface} terminal"
        );
    }
}

#[test]
fn typing_agents_shows_the_one_agent_row() {
    let mut composer = joined();
    type_text(&mut composer, "/agents");
    let menu = screen(&composer);
    // The input line shows what was typed; the menu rows show builtins.
    let rows = menu
        .lines()
        .map(str::trim_start)
        .filter(|line| line.starts_with("/agent") && !line.starts_with("/agents"))
        .count();
    assert_eq!(rows, 1, "one /agent row for /agents:\n{menu}");
}

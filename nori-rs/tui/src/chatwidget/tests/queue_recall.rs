//! Taking a queued message back into the composer, and keeping Alt+Up for
//! parent navigation only.

use super::*;
use crate::app_event::HarnessAction;
use crate::app_event::QueuedPromptAction;
use crate::app_event::ReclaimOutcome;
use pretty_assertions::assert_eq;

fn entry(id: &str, text: &str, editable: bool) -> nori_protocol::QueueEntry {
    nori_protocol::QueueEntry {
        id: id.to_string(),
        text: text.to_string(),
        editable,
    }
}

fn show_queue(chat: &mut ChatWidget, entries: Vec<nori_protocol::QueueEntry>) {
    let generation = chat.session_generation;
    chat.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::QueueChanged(
            nori_protocol::QueueSnapshot {
                prompts: entries.iter().map(|entry| entry.text.clone()).collect(),
                entries,
                paused: None,
            },
        )),
    );
}

fn reclaim_requests(rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>) -> Vec<String> {
    std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|event| match event {
            AppEvent::HarnessAction(HarnessAction::ReclaimQueuedPrompt { event_id, .. }) => {
                Some(event_id)
            }
            _ => None,
        })
        .collect()
}

fn queued_rows(chat: &ChatWidget) -> Vec<String> {
    use crate::render::renderable::Renderable;
    let area = ratatui::layout::Rect::new(0, 0, 80, chat.bottom_pane.desired_height(80));
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    chat.bottom_pane.render(area, &mut buffer);
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .filter_map(|line| {
            line.trim_start()
                .strip_prefix("↳ ")
                .map(|row| row.trim_end().to_string())
        })
        .collect()
}

fn press(chat: &mut ChatWidget, code: KeyCode, modifiers: KeyModifiers) {
    chat.handle_key_event(KeyEvent::new(code, modifiers));
}

fn three_queued(chat: &mut ChatWidget) {
    show_queue(
        chat,
        vec![
            entry("a", "alpha", true),
            entry("b", "dup", true),
            entry("c", "dup", true),
        ],
    );
}

/// The dim key hint drawn under an unfocused queue, if any.
fn queue_hint(chat: &ChatWidget) -> Option<String> {
    use crate::render::renderable::Renderable;
    let area = ratatui::layout::Rect::new(0, 0, 80, chat.bottom_pane.desired_height(80));
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    chat.bottom_pane.render(area, &mut buffer);
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .find_map(|line| {
            line.trim()
                .starts_with('↑')
                .then(|| line.trim().to_string())
        })
}

/// Every history line inserted since the last drain, as one string.
fn history_text(rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>) -> String {
    drain_insert_history(rx)
        .iter()
        .map(|lines| lines_to_single_string(lines))
        .collect()
}

#[test]
fn an_edited_message_stays_hidden_until_the_queue_confirms_it_left() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    three_queued(&mut chat);
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up, KeyModifiers::NONE);
    press(&mut chat, KeyCode::Enter, KeyModifiers::NONE);

    assert_eq!(reclaim_requests(&mut rx), ["c"]);
    assert_eq!(queued_rows(&chat), ["alpha", "dup"]);

    chat.on_queued_prompt_reclaimed(
        "c".to_string(),
        QueuedPromptAction::Edit,
        ReclaimOutcome::Reclaimed("dup".to_string()),
    );
    assert_eq!(chat.composer_text(), "dup");
    assert_eq!(
        queued_rows(&chat),
        ["alpha", "dup"],
        "the row stays hidden while the confirming snapshot is still on its way"
    );

    show_queue(
        &mut chat,
        vec![entry("a", "alpha", true), entry("b", "dup", true)],
    );
    assert_eq!(queued_rows(&chat), ["alpha", "dup"]);
}

#[test]
fn plain_up_with_a_draft_stays_in_the_composer() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    three_queued(&mut chat);
    chat.set_composer_text("draft".to_string());
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up, KeyModifiers::NONE);

    assert_eq!(reclaim_requests(&mut rx), Vec::<String>::new());
    assert_eq!(queued_rows(&chat), ["alpha", "dup", "dup"]);
}

#[test]
fn a_queue_without_ids_from_an_older_host_is_shown_but_cannot_be_selected() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    let generation = chat.session_generation;
    chat.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::QueueChanged(
            nori_protocol::QueueSnapshot {
                prompts: vec!["legacy".to_string()],
                entries: Vec::new(),
                paused: None,
            },
        )),
    );
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up, KeyModifiers::NONE);
    assert_eq!(queued_rows(&chat), ["legacy"], "nothing is selected");
    press(&mut chat, KeyCode::Enter, KeyModifiers::NONE);

    assert_eq!(reclaim_requests(&mut rx), Vec::<String>::new());
    assert_eq!(queued_rows(&chat), ["legacy"]);
}

#[test]
fn an_edit_that_fails_to_reach_the_harness_shows_the_row_again() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    show_queue(&mut chat, vec![entry("a", "alpha", true)]);
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up, KeyModifiers::NONE);
    press(&mut chat, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(queued_rows(&chat), Vec::<String>::new());
    chat.on_queued_prompt_reclaimed(
        "a".to_string(),
        QueuedPromptAction::Edit,
        ReclaimOutcome::Failed,
    );

    assert_eq!(queued_rows(&chat), ["alpha"]);
    assert_eq!(chat.composer_text(), "");
    let history = history_text(&mut rx);
    assert!(!history.contains("already started"), "{history:?}");
}

#[test]
fn alt_up_at_the_root_leaves_the_queue_and_an_empty_composer_alone() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    three_queued(&mut chat);
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up, KeyModifiers::ALT);

    assert_eq!(reclaim_requests(&mut rx), Vec::<String>::new());
    assert_eq!(queued_rows(&chat), ["alpha", "dup", "dup"]);
    assert_eq!(chat.composer_text(), "");
    type_text(&mut chat, "x");
    assert_eq!(
        chat.composer_text(),
        "x",
        "the list did not take focus, so typing reaches the composer"
    );
}

#[test]
fn alt_up_at_the_root_keeps_a_draft_and_the_queue() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    three_queued(&mut chat);
    chat.set_composer_text("half-typed".to_string());
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up, KeyModifiers::ALT);

    assert_eq!(reclaim_requests(&mut rx), Vec::<String>::new());
    assert_eq!(queued_rows(&chat), ["alpha", "dup", "dup"]);
    assert_eq!(chat.composer_text(), "half-typed");
}

#[test]
fn alt_up_at_the_root_does_not_recall_an_earlier_prompt() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    type_text(&mut chat, "earlier prompt");
    press(&mut chat, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(chat.composer_text(), "");
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up, KeyModifiers::ALT);
    assert_eq!(chat.composer_text(), "");

    press(&mut chat, KeyCode::Up, KeyModifiers::NONE);
    assert_eq!(
        chat.composer_text(),
        "earlier prompt",
        "plain Up still recalls the earlier prompt"
    );
}

#[test]
fn the_queue_hint_offers_only_up_to_select() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    three_queued(&mut chat);
    while rx.try_recv().is_ok() {}

    let hint = queue_hint(&chat);

    assert_eq!(
        hint.as_deref(),
        Some("↑ select"),
        "the unfocused queue offers Up to select and nothing else"
    );
}

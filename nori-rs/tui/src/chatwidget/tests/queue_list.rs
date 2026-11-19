//! Selecting a queued message in the list above the composer, and resuming a
//! queue held after an interrupted or failed turn.

use super::*;
use crate::app_event::HarnessAction;
use crate::app_event::QueuedPromptAction;
use crate::app_event::ReclaimOutcome;
use nori_protocol::QueuePause;
use pretty_assertions::assert_eq;

fn entry(id: &str, text: &str, editable: bool) -> nori_protocol::QueueEntry {
    nori_protocol::QueueEntry {
        id: id.to_string(),
        text: text.to_string(),
        editable,
    }
}

fn show_queue(
    chat: &mut ChatWidget,
    entries: Vec<nori_protocol::QueueEntry>,
    paused: Option<QueuePause>,
) {
    let generation = chat.session_generation;
    chat.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::QueueChanged(
            nori_protocol::QueueSnapshot {
                prompts: entries.iter().map(|entry| entry.text.clone()).collect(),
                entries,
                paused,
            },
        )),
    );
}

fn three_queued(chat: &mut ChatWidget, paused: Option<QueuePause>) {
    show_queue(
        chat,
        vec![
            entry("a", "alpha", true),
            entry("b", "dup", true),
            entry("c", "dup", true),
        ],
        paused,
    );
}

/// Every harness request the widget sent, in order.
fn harness_requests(rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>) -> Vec<String> {
    std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|event| match event {
            AppEvent::HarnessAction(HarnessAction::ReclaimQueuedPrompt { event_id, action }) => {
                Some(format!("{action:?} {event_id}"))
            }
            AppEvent::HarnessAction(HarnessAction::ResumeQueue) => Some("Resume".to_string()),
            AppEvent::HarnessAction(HarnessAction::Cancel) => Some("Cancel".to_string()),
            _ => None,
        })
        .collect()
}

fn queue_lines(chat: &ChatWidget) -> Vec<String> {
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
                .to_string()
        })
        .collect()
}

/// Queued rows as drawn, with the selected one marked `>`.
fn rows(chat: &ChatWidget) -> Vec<String> {
    queue_lines(chat)
        .into_iter()
        .filter_map(|line| {
            line.strip_prefix("  ↳ ")
                .map(ToString::to_string)
                .or_else(|| line.strip_prefix("  › ").map(|row| format!("> {row}")))
        })
        .collect()
}

fn press(chat: &mut ChatWidget, code: KeyCode) {
    chat.handle_key_event(KeyEvent::new(code, KeyModifiers::NONE));
}

/// Type `text` as key presses and let the composer settle them.
fn type_text(chat: &mut ChatWidget, text: &str) {
    for c in text.chars() {
        press(chat, KeyCode::Char(c));
    }
    std::thread::sleep(
        crate::bottom_pane::ChatComposer::recommended_paste_flush_delay()
            + std::time::Duration::from_millis(5),
    );
    chat.bottom_pane.flush_paste_burst_if_due();
}

#[test]
fn up_on_an_empty_composer_selects_the_newest_message_without_taking_it() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    three_queued(&mut chat, None);
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up);

    assert_eq!(harness_requests(&mut rx), Vec::<String>::new());
    assert_eq!(rows(&chat), ["alpha", "dup", "> dup"]);
}

#[test]
fn enter_edits_the_selected_duplicate_by_its_id() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    three_queued(&mut chat, None);
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up);
    press(&mut chat, KeyCode::Char('k'));
    assert_eq!(rows(&chat), ["alpha", "> dup", "dup"]);
    press(&mut chat, KeyCode::Enter);

    assert_eq!(harness_requests(&mut rx), ["Edit b"]);
    assert_eq!(rows(&chat), ["alpha", "dup"], "the list closes on edit");
    chat.on_queued_prompt_reclaimed(
        "b".to_string(),
        QueuedPromptAction::Edit,
        ReclaimOutcome::Reclaimed("dup".to_string()),
    );
    assert_eq!(chat.composer_text(), "dup");
}

#[test]
fn e_edits_the_selected_message_too() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    three_queued(&mut chat, None);
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up);
    press(&mut chat, KeyCode::Up);
    press(&mut chat, KeyCode::Up);
    press(&mut chat, KeyCode::Char('e'));

    assert_eq!(harness_requests(&mut rx), ["Edit a"]);
}

#[test]
fn selection_skips_messages_that_cannot_be_edited() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    show_queue(
        &mut chat,
        vec![
            entry("a", "alpha", true),
            entry("img", "with image", false),
            entry("c", "gamma", true),
        ],
        None,
    );
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up);
    press(&mut chat, KeyCode::Char('k'));
    assert_eq!(rows(&chat), ["> alpha", "with image", "gamma"]);
    press(&mut chat, KeyCode::Char('k'));
    assert_eq!(rows(&chat), ["> alpha", "with image", "gamma"]);
    press(&mut chat, KeyCode::Char('j'));
    assert_eq!(rows(&chat), ["alpha", "with image", "> gamma"]);
}

#[test]
fn up_with_only_uneditable_messages_stays_in_the_composer() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    show_queue(&mut chat, vec![entry("img", "with image", false)], None);
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up);
    type_text(&mut chat, "x");

    assert_eq!(harness_requests(&mut rx), Vec::<String>::new());
    assert_eq!(chat.composer_text(), "x");
}

#[test]
fn delete_removes_only_the_selected_message_and_keeps_the_list_open() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    three_queued(&mut chat, None);
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up);
    press(&mut chat, KeyCode::Char('k'));
    press(&mut chat, KeyCode::Char('x'));

    assert_eq!(harness_requests(&mut rx), ["Remove b"]);
    assert_eq!(rows(&chat), ["alpha", "> dup"]);

    chat.on_queued_prompt_reclaimed(
        "b".to_string(),
        QueuedPromptAction::Remove,
        ReclaimOutcome::Reclaimed("dup".to_string()),
    );
    show_queue(
        &mut chat,
        vec![entry("a", "alpha", true), entry("c", "dup", true)],
        None,
    );
    assert_eq!(chat.composer_text(), "", "a removed message is discarded");
    assert_eq!(rows(&chat), ["alpha", "> dup"]);

    press(&mut chat, KeyCode::Delete);
    assert_eq!(harness_requests(&mut rx), ["Remove c"]);
    assert_eq!(rows(&chat), ["> alpha"]);
}

#[test]
fn a_removal_answered_after_the_user_started_typing_keeps_the_draft() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    three_queued(&mut chat, None);
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up);
    press(&mut chat, KeyCode::Char('x'));
    press(&mut chat, KeyCode::Esc);
    type_text(&mut chat, "draft");
    chat.on_queued_prompt_reclaimed(
        "c".to_string(),
        QueuedPromptAction::Remove,
        ReclaimOutcome::Reclaimed("dup".to_string()),
    );

    assert_eq!(harness_requests(&mut rx), ["Remove c"]);
    assert_eq!(chat.composer_text(), "draft");
}

#[test]
fn a_removal_that_lost_the_race_to_dispatch_says_so() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    show_queue(&mut chat, vec![entry("a", "alpha", true)], None);
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up);
    press(&mut chat, KeyCode::Char('x'));
    chat.on_queued_prompt_reclaimed(
        "a".to_string(),
        QueuedPromptAction::Remove,
        ReclaimOutcome::AlreadyStarted,
    );

    let history = drain_insert_history(&mut rx)
        .iter()
        .map(|lines| lines_to_single_string(lines))
        .collect::<String>();
    assert!(
        history.contains("already started"),
        "the user learns it was sent: {history:?}"
    );
    assert_eq!(chat.composer_text(), "");
}

#[test]
fn esc_leaves_the_list_without_interrupting_the_turn() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    chat.on_task_started();
    three_queued(&mut chat, None);
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up);
    assert!(!chat.is_normal_backtrack_mode());
    press(&mut chat, KeyCode::Esc);

    assert_eq!(harness_requests(&mut rx), Vec::<String>::new());
    assert_eq!(rows(&chat), ["alpha", "dup", "dup"]);
}

#[test]
fn typing_leaves_the_list_and_reaches_the_composer() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    three_queued(&mut chat, None);
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up);
    type_text(&mut chat, "hi");

    assert_eq!(chat.composer_text(), "hi");
    assert_eq!(rows(&chat), ["alpha", "dup", "dup"]);
    assert_eq!(harness_requests(&mut rx), Vec::<String>::new());
}

/// The head `dup` is selected while two identical `dup` rows wait behind it.
fn select_the_head_of_identical_rows(chat: &mut ChatWidget) {
    show_queue(
        chat,
        vec![
            entry("a", "dup", true),
            entry("b", "dup", true),
            entry("c", "tail", true),
        ],
        None,
    );
    press(chat, KeyCode::Up);
    press(chat, KeyCode::Up);
    press(chat, KeyCode::Up);
    assert_eq!(rows(chat), ["> dup", "dup", "tail"]);
}

fn head_starts(chat: &mut ChatWidget) {
    show_queue(
        chat,
        vec![entry("b", "dup", true), entry("c", "tail", true)],
        None,
    );
}

#[test]
fn delete_after_the_selected_message_starts_waits_for_a_fresh_pick() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    select_the_head_of_identical_rows(&mut chat);
    while rx.try_recv().is_ok() {}

    head_starts(&mut chat);
    assert_eq!(rows(&chat), ["dup", "tail"], "nothing is selected now");
    press(&mut chat, KeyCode::Char('x'));
    press(&mut chat, KeyCode::Delete);

    assert_eq!(harness_requests(&mut rx), Vec::<String>::new());
    assert_eq!(rows(&chat), ["dup", "tail"], "the neighbour survives");
    assert_eq!(chat.composer_text(), "");

    press(&mut chat, KeyCode::Char('j'));
    assert_eq!(rows(&chat), ["> dup", "tail"]);
    press(&mut chat, KeyCode::Char('x'));
    assert_eq!(harness_requests(&mut rx), ["Remove b"]);
}

#[test]
fn edit_after_the_selected_message_starts_waits_for_a_fresh_pick() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    select_the_head_of_identical_rows(&mut chat);
    while rx.try_recv().is_ok() {}

    head_starts(&mut chat);
    press(&mut chat, KeyCode::Enter);
    press(&mut chat, KeyCode::Char('e'));

    assert_eq!(harness_requests(&mut rx), Vec::<String>::new());
    assert_eq!(chat.composer_text(), "", "nothing lands in the composer");
    assert_eq!(rows(&chat), ["dup", "tail"]);

    press(&mut chat, KeyCode::Char('k'));
    assert_eq!(rows(&chat), ["> dup", "tail"]);
    press(&mut chat, KeyCode::Enter);
    assert_eq!(harness_requests(&mut rx), ["Edit b"]);
}

#[test]
fn the_list_says_why_the_selection_was_cleared() {
    let (mut chat, _rx, _unused_rx) = make_chatwidget_manual();
    select_the_head_of_identical_rows(&mut chat);

    head_starts(&mut chat);

    let lines = queue_lines(&chat);
    assert!(
        lines
            .iter()
            .any(|line| line.contains("selected message started")),
        "{lines:#?}"
    );
}

#[test]
fn the_selection_stays_on_its_message_when_another_one_starts() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    three_queued(&mut chat, None);
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up);
    assert_eq!(rows(&chat), ["alpha", "dup", "> dup"]);
    show_queue(
        &mut chat,
        vec![entry("b", "dup", true), entry("c", "dup", true)],
        None,
    );

    assert_eq!(rows(&chat), ["dup", "> dup"]);
    press(&mut chat, KeyCode::Char('x'));
    assert_eq!(harness_requests(&mut rx), ["Remove c"]);
}

#[test]
fn esc_closes_a_list_whose_selection_was_cleared() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    select_the_head_of_identical_rows(&mut chat);
    head_starts(&mut chat);
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Esc);
    type_text(&mut chat, "x");

    assert_eq!(harness_requests(&mut rx), Vec::<String>::new());
    assert_eq!(chat.composer_text(), "x");
}

#[test]
fn the_list_closes_when_its_last_message_starts() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    show_queue(&mut chat, vec![entry("a", "alpha", true)], None);
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up);
    show_queue(&mut chat, Vec::new(), None);
    type_text(&mut chat, "x");

    assert_eq!(harness_requests(&mut rx), Vec::<String>::new());
    assert_eq!(chat.composer_text(), "x");
}

#[test]
fn an_interrupted_queue_shows_it_is_paused_with_its_count() {
    let (mut chat, _rx, _unused_rx) = make_chatwidget_manual();
    three_queued(&mut chat, Some(QueuePause::Interrupted));

    let lines = queue_lines(&chat);
    assert!(
        lines
            .iter()
            .any(|line| line.contains("3 queued") && line.contains("paused")),
        "{lines:#?}"
    );
    assert_eq!(rows(&chat), ["alpha", "dup", "dup"]);
}

#[test]
fn enter_on_an_empty_composer_resumes_a_paused_queue() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    three_queued(&mut chat, Some(QueuePause::Interrupted));
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Enter);

    assert_eq!(harness_requests(&mut rx), ["Resume"]);
    assert!(chat.initial_user_message.is_none());
}

#[test]
fn enter_with_a_draft_sends_the_draft_and_does_not_resume() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    three_queued(&mut chat, Some(QueuePause::Interrupted));
    chat.set_composer_text("new question".to_string());
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Enter);

    assert_eq!(harness_requests(&mut rx), Vec::<String>::new());
    let submitted = chat.initial_user_message.take().expect("submitted message");
    assert_eq!(submitted.text, "new question");
}

#[test]
fn enter_on_an_empty_composer_does_nothing_when_the_queue_is_not_paused() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    three_queued(&mut chat, None);
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Enter);

    assert_eq!(harness_requests(&mut rx), Vec::<String>::new());
}

#[test]
fn a_paused_queue_can_still_be_edited_from_the_list() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    three_queued(&mut chat, Some(QueuePause::TurnFailed));
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up);
    press(&mut chat, KeyCode::Char('k'));
    press(&mut chat, KeyCode::Char('x'));
    press(&mut chat, KeyCode::Esc);
    press(&mut chat, KeyCode::Enter);

    assert_eq!(harness_requests(&mut rx), ["Remove b", "Resume"]);
}

#[test]
fn a_follow_only_attachment_can_neither_select_nor_resume() {
    let (mut chat, mut rx, _unused_rx) = make_cloud_chatwidget_manual();
    let generation = chat.session_generation;
    chat.handle_session_event(
        generation,
        super::part10::initialize_follow_only_agent_event(),
    );
    three_queued(&mut chat, Some(QueuePause::Interrupted));
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up);
    press(&mut chat, KeyCode::Char('x'));
    chat.set_composer_text(String::new());
    press(&mut chat, KeyCode::Enter);

    assert_eq!(harness_requests(&mut rx), Vec::<String>::new());
    assert_eq!(rows(&chat), ["alpha", "dup", "dup"]);
}

#[test]
fn enter_on_an_empty_composer_resumes_while_a_turn_is_running() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    chat.on_task_started();
    three_queued(&mut chat, Some(QueuePause::Interrupted));
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Enter);

    assert_eq!(harness_requests(&mut rx), ["Resume"]);
}

#[test]
fn an_edit_that_lost_the_race_to_dispatch_keeps_the_draft() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    three_queued(&mut chat, None);
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up);
    press(&mut chat, KeyCode::Enter);
    type_text(&mut chat, "draft");
    chat.on_queued_prompt_reclaimed(
        "c".to_string(),
        QueuedPromptAction::Edit,
        ReclaimOutcome::AlreadyStarted,
    );

    assert_eq!(harness_requests(&mut rx), ["Edit c"]);
    assert_eq!(chat.composer_text(), "draft");
}

#[test]
fn a_pasted_line_break_does_not_resume_a_paused_queue() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    three_queued(&mut chat, Some(QueuePause::Interrupted));
    while rx.try_recv().is_ok() {}

    for c in "foo".chars() {
        press(&mut chat, KeyCode::Char(c));
    }
    press(&mut chat, KeyCode::Enter);

    assert_eq!(harness_requests(&mut rx), Vec::<String>::new());
}

#[test]
fn a_bracketed_paste_leaves_the_list() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    three_queued(&mut chat, None);
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up);
    chat.handle_paste("pasted".to_string());
    type_text(&mut chat, "x");

    assert_eq!(harness_requests(&mut rx), Vec::<String>::new());
    assert_eq!(chat.composer_text(), "pastedx");
}

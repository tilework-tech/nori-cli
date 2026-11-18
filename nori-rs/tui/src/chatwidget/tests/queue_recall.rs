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

#[test]
fn alt_up_takes_back_only_the_newest_queued_message() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    three_queued(&mut chat);
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up, KeyModifiers::ALT);

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
fn a_second_press_before_the_first_answer_takes_the_next_message() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    three_queued(&mut chat);
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up, KeyModifiers::ALT);
    press(&mut chat, KeyCode::Up, KeyModifiers::ALT);

    assert_eq!(reclaim_requests(&mut rx), ["c", "b"]);
    assert_eq!(queued_rows(&chat), ["alpha"]);

    chat.on_queued_prompt_reclaimed(
        "c".to_string(),
        QueuedPromptAction::Edit,
        ReclaimOutcome::Reclaimed("dup two".to_string()),
    );
    chat.on_queued_prompt_reclaimed(
        "b".to_string(),
        QueuedPromptAction::Edit,
        ReclaimOutcome::Reclaimed("dup one".to_string()),
    );

    assert_eq!(
        chat.composer_text(),
        "dup one\ndup two",
        "reclaimed messages keep their queue order"
    );
}

#[test]
fn alt_up_keeps_an_existing_draft_after_the_reclaimed_message() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    three_queued(&mut chat);
    chat.set_composer_text("half-typed".to_string());
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up, KeyModifiers::ALT);
    assert_eq!(reclaim_requests(&mut rx), ["c"]);
    chat.on_queued_prompt_reclaimed(
        "c".to_string(),
        QueuedPromptAction::Edit,
        ReclaimOutcome::Reclaimed("dup".to_string()),
    );

    assert_eq!(chat.composer_text(), "dup\nhalf-typed");
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
fn a_message_that_already_started_is_reported_and_nothing_is_lost() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    show_queue(&mut chat, vec![entry("a", "alpha", true)]);
    chat.set_composer_text("keep me".to_string());
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up, KeyModifiers::ALT);
    assert_eq!(reclaim_requests(&mut rx), ["a"]);
    chat.on_queued_prompt_reclaimed(
        "a".to_string(),
        QueuedPromptAction::Edit,
        ReclaimOutcome::AlreadyStarted,
    );

    assert_eq!(chat.composer_text(), "keep me");
    let history = drain_insert_history(&mut rx)
        .iter()
        .map(|lines| lines_to_single_string(lines))
        .collect::<String>();
    assert!(
        history.contains("already started"),
        "the user is told why nothing came back: {history:?}"
    );
}

#[test]
fn rows_that_cannot_be_edited_are_skipped_and_never_reclaimed() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    show_queue(
        &mut chat,
        vec![entry("a", "alpha", true), entry("img", "with image", false)],
    );
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up, KeyModifiers::ALT);

    assert_eq!(reclaim_requests(&mut rx), ["a"]);
    assert_eq!(queued_rows(&chat), ["with image"]);
}

#[test]
fn a_queue_without_ids_from_an_older_host_is_shown_but_not_reclaimed() {
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

    press(&mut chat, KeyCode::Up, KeyModifiers::ALT);

    assert_eq!(reclaim_requests(&mut rx), Vec::<String>::new());
    assert_eq!(queued_rows(&chat), ["legacy"]);
}

#[test]
fn a_reclaim_that_fails_to_reach_the_harness_shows_the_row_again() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    show_queue(&mut chat, vec![entry("a", "alpha", true)]);
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up, KeyModifiers::ALT);
    assert_eq!(queued_rows(&chat), Vec::<String>::new());
    chat.on_queued_prompt_reclaimed(
        "a".to_string(),
        QueuedPromptAction::Edit,
        ReclaimOutcome::Failed,
    );

    assert_eq!(queued_rows(&chat), ["alpha"]);
    assert_eq!(chat.composer_text(), "");
    let history = drain_insert_history(&mut rx)
        .iter()
        .map(|lines| lines_to_single_string(lines))
        .collect::<String>();
    assert!(!history.contains("already started"), "{history:?}");
}

#[test]
fn a_reclaimed_message_keeps_the_drafts_attached_image() {
    let (mut chat, mut rx, _unused_rx) = make_chatwidget_manual();
    three_queued(&mut chat);
    chat.set_composer_text("look at".to_string());
    let image = PathBuf::from("/tmp/queue-recall-draft.png");
    chat.attach_image(image.clone(), 32, 16, "PNG");
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up, KeyModifiers::ALT);
    assert_eq!(reclaim_requests(&mut rx), ["c"]);
    chat.on_queued_prompt_reclaimed(
        "c".to_string(),
        QueuedPromptAction::Edit,
        ReclaimOutcome::Reclaimed("dup".to_string()),
    );
    assert_eq!(
        chat.composer_text(),
        "dup\nlook at[queue-recall-draft.png 32x16]"
    );

    press(&mut chat, KeyCode::Enter, KeyModifiers::NONE);
    let submitted = chat.initial_user_message.take().expect("submitted message");
    assert_eq!(submitted.image_paths, vec![image]);
}

#[test]
fn a_follow_only_attachment_never_takes_the_owners_queued_message() {
    let (mut chat, mut rx, _unused_rx) = make_cloud_chatwidget_manual();
    let generation = chat.session_generation;
    chat.handle_session_event(
        generation,
        super::part10::initialize_follow_only_agent_event(),
    );
    three_queued(&mut chat);
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up, KeyModifiers::ALT);

    assert_eq!(reclaim_requests(&mut rx), Vec::<String>::new());
    assert_eq!(queued_rows(&chat), ["alpha", "dup", "dup"]);
}

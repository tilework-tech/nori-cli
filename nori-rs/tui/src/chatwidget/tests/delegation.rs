use super::*;

const PARENT: &str = "00000000-0000-0000-0000-000000000001";
const CHILD: &str = "00000000-0000-0000-0000-000000000002";

fn delegation(status: &str) -> nori_protocol::SessionEvent {
    delegation_with(status, "agent", "Review the parser", None)
}

fn delegation_with(
    status: &str,
    origin: &str,
    task: &str,
    result: Option<&str>,
) -> nori_protocol::SessionEvent {
    serde_json::from_value(
        serde_json::json!({"source":"nori","event":{"event_type":"delegation_changed","event":{
            "request_id":"delegation-one","parent_id":PARENT,
            "child_id":CHILD,"parent_agent":"mock-model","agent":"mock-model-alt",
            "task":task,"depth":1,"status":status,"result":result,"origin":origin
        }}}),
    )
    .unwrap()
}

/// Screen and transcript text of every history cell the widget emitted.
fn history_text(rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>) -> (String, String) {
    let mut screen = String::new();
    let mut transcript = String::new();
    while let Ok(event) = rx.try_recv() {
        if let AppEvent::InsertHistoryCell(cell) = event {
            screen.push_str(&lines_to_single_string(&cell.display_lines(100)));
            transcript.push_str(&lines_to_single_string(&cell.transcript_lines(100)));
        }
    }
    let trim = |text: String| {
        text.lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n")
    };
    (trim(screen), trim(transcript))
}

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

fn delegating_parent() -> (ChatWidget, tokio::sync::mpsc::UnboundedReceiver<AppEvent>) {
    let (mut chat, mut rx, _) = make_chatwidget_manual();
    chat.config.multi_session = true;
    while rx.try_recv().is_ok() {}
    (chat, rx)
}

#[test]
fn different_agent_approval_names_who_asks_which_agent_runs_and_the_task() {
    let (mut chat, _rx) = delegating_parent();
    let generation = chat.session_generation();
    chat.handle_session_event(generation, delegation("awaiting_approval"));
    let rendered = render_bottom_pane(&chat);
    assert!(
        !rendered.contains(PARENT) && !rendered.contains(CHILD),
        "the prompt names sessions, not raw IDs: {rendered}"
    );
    for expected in [
        "asked by",
        "mock-model (this session)",
        "mock-model-alt",
        "Review the parser",
    ] {
        assert!(rendered.contains(expected), "{expected}: {rendered}");
    }
    insta::assert_snapshot!("local_delegation_approval", rendered);
}

#[test]
fn user_requested_approval_says_the_user_asked() {
    let (mut chat, _rx) = delegating_parent();
    let generation = chat.session_generation();
    chat.handle_session_event(
        generation,
        delegation_with("awaiting_approval", "user", "Review the parser", None),
    );
    let rendered = render_bottom_pane(&chat);
    assert!(rendered.contains("you, with /delegate"), "{rendered}");
    insta::assert_snapshot!("user_delegation_approval", rendered);
}

#[test]
fn escape_declines_a_pending_delegation() {
    let (mut chat, mut rx) = delegating_parent();
    let generation = chat.session_generation();
    chat.handle_session_event(generation, delegation("awaiting_approval"));
    chat.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Esc,
        crossterm::event::KeyModifiers::NONE,
    ));
    let mut resolved = None;
    while let Ok(event) = rx.try_recv() {
        if let AppEvent::ResolveDelegation { approved, .. } = event {
            resolved = Some(approved);
        }
    }
    assert_eq!(resolved, Some(false));
}

#[test]
fn running_delegation_is_one_line_naming_child_agent_and_status() {
    let (mut chat, mut rx) = delegating_parent();
    let generation = chat.session_generation();
    chat.handle_session_event(
        generation,
        delegation_with(
            "running",
            "agent",
            "Write snapshot tests for TreeRow\nCover depths 0 to 3. Do not touch picker.rs.",
            None,
        ),
    );
    let (screen, transcript) = history_text(&mut rx);
    assert!(
        !screen.contains(CHILD) && !screen.contains(PARENT),
        "{screen}"
    );
    assert!(
        !screen.contains("Do not touch picker.rs"),
        "the brief stays off screen: {screen}"
    );
    assert!(
        transcript.contains("Cover depths 0 to 3. Do not touch picker.rs."),
        "the transcript view shows the whole brief: {transcript}"
    );
    let card = screen
        .lines()
        .find(|line| line.contains("mock-model-alt"))
        .unwrap_or_else(|| panic!("card missing: {screen}"));
    assert!(
        card.contains(" · started") && !card.contains("working"),
        "a card keeps a word that stays true once the child moves on: {screen}"
    );
    insta::assert_snapshot!("running_delegation_line", screen);
    insta::assert_snapshot!("running_delegation_transcript", transcript);
}

#[test]
fn user_started_delegation_is_attributed_to_the_user() {
    let (mut chat, mut rx) = delegating_parent();
    let generation = chat.session_generation();
    chat.handle_session_event(
        generation,
        delegation_with("running", "user", "Review the parser", None),
    );
    let (screen, _) = history_text(&mut rx);
    assert!(screen.contains("you delegated"), "{screen}");
}

#[test]
fn completed_delegation_shows_a_result_block_naming_the_child() {
    let (mut chat, mut rx) = delegating_parent();
    let generation = chat.session_generation();
    chat.handle_session_event(generation, delegation("running"));
    while rx.try_recv().is_ok() {}
    chat.handle_session_event(
        generation,
        delegation_with(
            "completed",
            "agent",
            "Review the parser",
            Some("Found two bugs in parse_command.\nBoth have regression tests now."),
        ),
    );
    let (screen, _) = history_text(&mut rx);
    assert!(screen.contains("result from mock-model-alt"), "{screen}");
    assert!(screen.contains("Review the parser"), "{screen}");
    assert!(
        screen.contains("Both have regression tests now."),
        "{screen}"
    );
    assert!(!screen.contains(CHILD), "{screen}");
    insta::assert_snapshot!("completed_delegation_result", screen);
}

#[test]
fn long_results_are_shortened_on_screen_and_kept_whole_in_the_transcript() {
    let (mut chat, mut rx) = delegating_parent();
    let generation = chat.session_generation();
    let result = (1..=12)
        .map(|n| format!("finding {n}"))
        .collect::<Vec<_>>()
        .join("\n");
    chat.handle_session_event(
        generation,
        delegation_with("completed", "agent", "Review the parser", Some(&result)),
    );
    let (screen, transcript) = history_text(&mut rx);
    assert!(!screen.contains("finding 12"), "{screen}");
    assert!(screen.contains("more lines"), "{screen}");
    assert!(transcript.contains("finding 12"), "{transcript}");
}

#[test]
fn long_result_lines_keep_the_result_rail_when_wrapped() {
    let (mut chat, mut rx) = delegating_parent();
    let generation = chat.session_generation();
    let long = "word ".repeat(40);
    chat.handle_session_event(
        generation,
        delegation_with("completed", "agent", "Review the parser", Some(long.trim())),
    );
    let (screen, _) = history_text(&mut rx);
    let rail_lines = screen
        .lines()
        .filter(|line| line.starts_with("  ▏ "))
        .count();
    assert!(
        rail_lines >= 2,
        "wrapped result lines keep the rail: {screen}"
    );
}

#[test]
fn a_user_message_that_only_starts_like_a_result_stays_whole() {
    let (mut chat, mut rx) = delegating_parent();
    chat.on_task_started();
    chat.handle_client_event(crate::presentation::ClientEvent::MessageDelta(
        crate::presentation::MessageDelta {
            stream: crate::presentation::MessageStream::User,
            delta: "[Nori delegation result] what does this header mean?\nPlease explain.".into(),
            message_id: Some("question".into()),
        },
    ));
    chat.on_task_complete(None);
    let (screen, _) = history_text(&mut rx);
    assert!(screen.contains("Please explain."), "{screen}");
}

#[test]
fn failed_declined_and_cancelled_delegations_read_clearly() {
    let mut rendered = Vec::new();
    for (status, result) in [
        ("failed", "agent exited (code 1)"),
        ("declined", "The user declined this delegation."),
        ("cancelled", "Session closed by the user"),
    ] {
        let (mut chat, mut rx) = delegating_parent();
        let generation = chat.session_generation();
        chat.handle_session_event(
            generation,
            delegation_with(status, "agent", "Review the parser", Some(result)),
        );
        let (screen, _) = history_text(&mut rx);
        assert!(
            screen.contains("mock-model-alt") && screen.contains(status) && !screen.contains(CHILD),
            "{screen}"
        );
        rendered.push(screen);
    }
    insta::assert_snapshot!("ended_delegation_lines", rendered.join("\n---\n"));
}

#[test]
fn delivered_result_reads_as_a_short_line_in_the_parent() {
    let (mut chat, mut rx) = delegating_parent();
    chat.on_task_started();
    chat.handle_client_event(crate::presentation::ClientEvent::MessageDelta(
        crate::presentation::MessageDelta {
            stream: crate::presentation::MessageStream::User,
            delta: nori_protocol::delegation_result_prompt(
                &serde_json::from_value::<nori_protocol::Delegation>(serde_json::json!({
                    "request_id":"delegation-one","parent_id":PARENT,"child_id":CHILD,
                    "parent_agent":"mock-model","agent":"mock-model-alt","task":"Review the parser",
                    "depth":1,"status":"completed","result":"Found two bugs."
                }))
                .unwrap(),
            ),
            message_id: Some("delegation-result".into()),
        },
    ));
    chat.on_task_complete(Some("Thanks".into()));
    let (screen, transcript) = history_text(&mut rx);
    assert!(!screen.contains(CHILD), "{screen}");
    assert!(screen.contains("mock-model-alt"), "{screen}");
    assert!(transcript.contains("Found two bugs."), "{transcript}");
    insta::assert_snapshot!("delivered_delegation_result", screen);
}

#[test]
fn a_delivered_result_fits_one_row_at_80_columns() {
    let (mut chat, mut rx) = delegating_parent();
    let task = "mock:request-permission:review-the-lexer-tokens";
    chat.on_task_started();
    chat.handle_client_event(crate::presentation::ClientEvent::MessageDelta(
        crate::presentation::MessageDelta {
            stream: crate::presentation::MessageStream::User,
            delta: nori_protocol::delegation_result_prompt(
                &serde_json::from_value::<nori_protocol::Delegation>(serde_json::json!({
                    "request_id":"delegation-one","parent_id":PARENT,"child_id":CHILD,
                    "parent_agent":"mock-model","agent":"mock-model-alt","task":task,
                    "depth":1,"status":"completed","result":"Found two bugs."
                }))
                .unwrap(),
            ),
            message_id: Some("delegation-result".into()),
        },
    ));
    chat.on_task_complete(Some("Thanks".into()));
    let mut rows = Vec::new();
    while let Ok(event) = rx.try_recv() {
        if let AppEvent::InsertHistoryCell(cell) = event {
            let text = lines_to_single_string(&cell.display_lines(80));
            if text.contains("delegation result") {
                rows = text.lines().map(str::trim_end).map(str::to_owned).collect();
            }
        }
    }
    let rows: Vec<String> = rows.into_iter().filter(|row| !row.is_empty()).collect();
    assert_eq!(rows.len(), 1, "{rows:#?}");
    assert!(rows[0].contains("mock:request"), "{}", rows[0]);
    assert!(rows[0].contains("lexer-tokens"), "{}", rows[0]);
    assert!(rows[0].contains("· done"), "{}", rows[0]);
}

#[test]
fn child_shutdown_preserves_terminal_and_history() {
    let (mut chat, mut rx, _) = make_chatwidget_manual();
    chat.config.multi_session = true;
    let id =
        nori_harness::ConversationId::from_string("00000000-0000-0000-0000-000000000002").unwrap();
    chat.managed_conversation_id = Some(id);
    chat.delegated_child = true;
    while rx.try_recv().is_ok() {}
    chat.handle_session_event(
        chat.session_generation(),
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::SessionEnded(
            nori_protocol::SessionEnded {
                reason: nori_protocol::SessionEndReason::Shutdown,
                message: None,
            },
        )),
    );
    while let Ok(event) = rx.try_recv() {
        assert!(
            !matches!(
                event,
                AppEvent::ExitRequest | AppEvent::SessionClosed { .. }
            ),
            "a completed child must not exit or replace the visible session"
        );
    }
}

#[test]
fn local_delegate_rejects_flag_off_without_prompting_agent() {
    let (mut chat, mut rx, _) = make_chatwidget_manual();
    chat.submit_user_message_text("/delegate mock-model-alt inspect parser".into());
    let mut output = String::new();
    while let Ok(event) = rx.try_recv() {
        if let AppEvent::InsertHistoryCell(cell) = event {
            output.push_str(&lines_to_single_string(&cell.display_lines(120)));
        } else {
            assert!(
                !matches!(event, AppEvent::HarnessAction(_) | AppEvent::NewSession),
                "flag-off command cannot submit work"
            );
        }
    }
    assert!(output.contains("multi_session"), "{output}");
}

#[test]
fn forked_parent_refreshes_pending_approval_target() {
    let (mut chat, mut rx, _) = make_chatwidget_manual();
    chat.config.multi_session = true;
    let generation = chat.session_generation();
    chat.handle_session_event(generation, delegation("awaiting_approval"));
    let nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::DelegationChanged(mut changed)) =
        delegation("awaiting_approval")
    else {
        panic!("delegation fixture");
    };
    changed.parent_id = "00000000-0000-0000-0000-000000000003".into();
    chat.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::DelegationChanged(changed)),
    );
    chat.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('1'),
        crossterm::event::KeyModifiers::NONE,
    ));
    let mut approved = None;
    while let Ok(event) = rx.try_recv() {
        if let AppEvent::ResolveDelegation { parent, .. } = event {
            approved = Some(parent.to_string());
        }
    }
    assert_eq!(
        approved.as_deref(),
        Some("00000000-0000-0000-0000-000000000003")
    );
}

#[test]
fn delegation_result_and_parent_continuation_preserve_composer_draft() {
    let (mut chat, _rx, _) = make_chatwidget_manual();
    chat.config.multi_session = true;
    chat.set_composer_text("/sessions".into());
    let generation = chat.session_generation();
    chat.handle_session_event(generation, delegation("completed"));
    chat.on_task_started();
    chat.handle_client_event(crate::presentation::ClientEvent::MessageDelta(
        crate::presentation::MessageDelta {
            stream: crate::presentation::MessageStream::User,
            delta: "[Nori delegation result] Child completed".into(),
            message_id: Some("delegation-result".into()),
        },
    ));
    chat.on_task_complete(Some("Result received".into()));
    assert_eq!(chat.composer_text(), "/sessions");
}

#[test]
fn a_shown_delegation_request_takes_enter_from_the_draft_and_keeps_it() {
    let (mut chat, mut rx) = delegating_parent();
    chat.set_composer_text("my draft".into());
    let generation = chat.session_generation();
    chat.handle_session_event(generation, delegation("awaiting_approval"));
    chat.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE,
    ));
    let mut resolved = None;
    while let Ok(event) = rx.try_recv() {
        if let AppEvent::ResolveDelegation { approved, .. } = event {
            resolved = Some(approved);
        }
    }
    assert_eq!(resolved, Some(true), "enter answers the request");
    assert_eq!(
        chat.composer_text(),
        "my draft",
        "the draft is neither sent nor changed"
    );
}

#[test]
fn a_running_card_fits_one_row_at_80_columns() {
    let (mut chat, mut rx) = delegating_parent();
    let generation = chat.session_generation();
    let task = "shared prefix for the parser review: child side mock:request-permission:z";
    chat.handle_session_event(generation, delegation_with("running", "agent", task, None));
    let mut cards = Vec::new();
    while let Ok(event) = rx.try_recv() {
        if let AppEvent::InsertHistoryCell(cell) = event {
            cards.push(cell);
        }
    }
    let card = cards.first().expect("a card is inserted");
    let at_80 = lines_to_single_string(&card.display_lines(80));
    let header = at_80.lines().next().unwrap_or_default();
    assert!(
        header.contains("shared") && header.contains("permission:z") && header.ends_with("started"),
        "the header keeps both ends of the name and its status on one row:\n{at_80}"
    );
    assert!(
        unicode_width::UnicodeWidthStr::width(header.trim_end()) <= 80,
        "{at_80}"
    );
    let wide = lines_to_single_string(&card.display_lines(140));
    assert!(
        wide.contains(task),
        "the whole name shows when it fits:\n{wide}"
    );
}

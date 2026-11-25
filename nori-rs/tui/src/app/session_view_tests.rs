use super::super::session_deck::SessionDeck;
use super::*;

fn rendered(event: AppEvent) -> String {
    let AppEvent::InsertHistoryCell(cell) = event else {
        panic!("expected history cell");
    };
    cell.display_lines(80)
        .iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn action_failures_route_to_active_parked_candidate_and_discard_removed_generations() {
    let (mut active, mut active_rx, _) = crate::chatwidget::tests::make_scoped_chatwidget_manual();
    let (parked, mut parked_rx, _) = crate::chatwidget::tests::make_scoped_chatwidget_manual();
    let (candidate, _, _) = crate::chatwidget::tests::make_scoped_chatwidget_manual();
    let active_generation = active.session_generation();
    let parked_generation = parked.session_generation();
    let candidate_generation = candidate.session_generation();
    let mut candidate_error = None;
    let mut deck = SessionDeck::default();
    deck.park(
        parked_generation,
        ParkedSession {
            widget: Box::new(parked),
            transcript: Vec::new(),
        },
    );
    route_session_history_event(
        &mut active,
        &mut deck,
        None,
        active_generation,
        AppEvent::HarnessActionFailed("active failure".into()),
    );
    for (generation, message) in [
        (parked_generation, "parked failure"),
        (candidate_generation, "candidate failure"),
    ] {
        assert!(
            route_session_history_event(
                &mut active,
                &mut deck,
                Some((candidate_generation, &mut candidate_error)),
                generation,
                AppEvent::HarnessActionFailed(message.into())
            )
            .is_none(),
            "another session's failure cannot render on screen"
        );
    }
    let mut visible = Vec::new();
    for rx in [&mut active_rx, &mut parked_rx] {
        while let Ok(event) = rx.try_recv() {
            if let AppEvent::SessionHistory { generation, event } = event
                && let Some(event) =
                    route_session_history_event(&mut active, &mut deck, None, generation, *event)
            {
                visible.push(rendered(event));
            }
        }
    }
    assert_eq!(visible, vec!["■ active failure"]);
    let parked = deck.parked(parked_generation).unwrap();
    let text = parked
        .transcript
        .iter()
        .flat_map(|cell| cell.display_lines(80))
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(text, "■ parked failure");
    assert_eq!(candidate_error.as_deref(), Some("candidate failure"));
    deck.remove(parked_generation);
    assert!(
        route_session_history_event(
            &mut active,
            &mut deck,
            None,
            parked_generation,
            AppEvent::HarnessActionFailed("removed failure".into())
        )
        .is_none()
    );
}
fn assert_failure_follows_buffered_message(parked_target: bool) {
    let (mut active, mut active_rx, _) = crate::chatwidget::tests::make_scoped_chatwidget_manual();
    let (mut child, mut child_rx, _) = crate::chatwidget::tests::make_scoped_chatwidget_manual();
    let target = if parked_target {
        &mut child
    } else {
        &mut active
    };
    let generation = target.session_generation();
    target.handle_client_event(crate::presentation::ClientEvent::MessageDelta(
        crate::presentation::MessageDelta {
            stream: crate::presentation::MessageStream::User,
            delta: "question before failure".into(),
            message_id: Some("question-one".into()),
        },
    ));
    let mut deck = SessionDeck::default();
    if parked_target {
        deck.park(
            generation,
            ParkedSession {
                widget: Box::new(child),
                transcript: Vec::new(),
            },
        );
    }
    let mut visible = Vec::new();
    if let Some(event) = route_session_history_event(
        &mut active,
        &mut deck,
        None,
        generation,
        AppEvent::HarnessActionFailed("operation failed".into()),
    ) {
        visible.push(rendered(event));
    }
    for rx in [&mut active_rx, &mut child_rx] {
        while let Ok(event) = rx.try_recv() {
            if let AppEvent::SessionHistory { generation, event } = event
                && let Some(event) =
                    route_session_history_event(&mut active, &mut deck, None, generation, *event)
            {
                visible.push(rendered(event));
            }
        }
    }
    let text = if parked_target {
        assert!(
            visible.is_empty(),
            "parked history cannot appear in the active session"
        );
        deck.parked(generation)
            .unwrap()
            .transcript
            .iter()
            .flat_map(|cell| cell.display_lines(80))
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        visible.join("\n")
    };
    let question = text
        .find("question before failure")
        .expect("buffered user message is flushed");
    let error = text.find("operation failed").expect("real error renders");
    assert!(question < error, "chronological history: {text}");
}

#[test]
fn active_action_failure_preserves_buffered_message_order() {
    assert_failure_follows_buffered_message(false);
}

#[test]
fn parked_action_failure_preserves_buffered_message_order() {
    assert_failure_follows_buffered_message(true);
}

#[test]
fn history_queued_by_removed_generation_is_discarded() {
    let (mut active, _, _) = crate::chatwidget::tests::make_chatwidget_manual();
    let (removed, _, _) = crate::chatwidget::tests::make_chatwidget_manual();
    let event = AppEvent::InsertHistoryCell(Box::new(crate::history_cell::new_error_event(
        "late failure".into(),
    )));
    assert!(
        route_session_history_event(
            &mut active,
            &mut Default::default(),
            None,
            removed.session_generation(),
            event
        )
        .is_none()
    );
}

#[test]
fn candidate_startup_history_retains_its_routing() {
    let (mut active, _, _) = crate::chatwidget::tests::make_chatwidget_manual();
    let (candidate, _, _) = crate::chatwidget::tests::make_chatwidget_manual();
    let generation = candidate.session_generation();
    let mut error = None;
    assert!(matches!(
        route_session_history_event(
            &mut active,
            &mut Default::default(),
            Some((generation, &mut error)),
            generation,
            AppEvent::AgentConnecting {
                display_name: "candidate".into()
            }
        ),
        Some(AppEvent::AgentConnecting { .. })
    ));
}

#[test]
fn a_script_result_reaches_only_the_session_that_ran_it() {
    let (mut active, mut active_rx, _) = crate::chatwidget::tests::make_scoped_chatwidget_manual();
    let (parked, _, _) = crate::chatwidget::tests::make_scoped_chatwidget_manual();
    let active_generation = active.session_generation();
    let parked_generation = parked.session_generation();
    let mut deck = SessionDeck::default();
    deck.park(
        parked_generation,
        ParkedSession {
            widget: Box::new(parked),
            transcript: Vec::new(),
        },
    );
    let finished = |text: &str| AppEvent::ScriptExecutionComplete {
        name: "weather".into(),
        result: Ok(text.into()),
    };
    while active_rx.try_recv().is_ok() {}

    let for_parked = route_session_history_event(
        &mut active,
        &mut deck,
        None,
        parked_generation,
        finished("parked output"),
    );
    deck.remove(parked_generation);
    let for_closed = route_session_history_event(
        &mut active,
        &mut deck,
        None,
        parked_generation,
        finished("closed output"),
    );
    let for_active = route_session_history_event(
        &mut active,
        &mut deck,
        None,
        active_generation,
        finished("active output"),
    );

    assert!(for_parked.is_none(), "the parked session keeps its output");
    assert!(
        for_closed.is_none(),
        "a closed session's output goes nowhere"
    );
    assert!(
        matches!(for_active, Some(AppEvent::ScriptExecutionComplete { .. })),
        "the session on screen gets its own output"
    );
    assert!(
        active_rx.try_recv().is_err(),
        "nothing reached the session on screen's agent"
    );
}

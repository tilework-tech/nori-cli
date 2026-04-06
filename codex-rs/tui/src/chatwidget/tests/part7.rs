use super::*;

/// When the ACP backend suppresses the stale Completed (common case), the
/// next turn's real Completed must not be consumed as stale.
///
/// Sequence:
///   1. Started(A)    → task running
///   2. Aborted(A)    → task stopped (user pressed ESC), counter = 1
///   3. Started(B)    → counter reset to 0
///   4. Completed(B)  → should finalize turn B normally
///
/// Before the fix, the counter from step 2 was never drained (because the
/// ACP backend suppressed the stale Completed), so the real Completed in
/// step 4 was consumed as stale, leaving the spinner running forever.
#[test]
fn acp_suppressed_stale_should_not_block_next_turn_completion() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    // Start and interrupt turn A
    chat.on_task_started();
    drain_insert_history(&mut rx);
    chat.on_interrupted_turn(TurnAbortReason::Interrupted);
    drain_insert_history(&mut rx);

    // ACP backend suppresses the stale Completed → no on_task_complete call.

    // Start turn B
    chat.on_task_started();
    drain_insert_history(&mut rx);

    // Real Completed from turn B should finalize the turn.
    chat.on_task_complete(None);
    drain_insert_history(&mut rx);

    // Task should be stopped.
    assert!(
        !chat.bottom_pane.is_task_running(),
        "Task should be stopped after real Completed"
    );
    assert!(
        chat.turn_finished,
        "Turn should be marked finished after real Completed"
    );
}

/// Multiple consecutive interrupts where ACP suppresses all stale Completeds.
/// The final real turn's Completed must still finalize normally.
#[test]
fn multiple_interrupts_with_acp_suppression_should_not_hang() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    // Interrupt twice in a row
    chat.on_task_started();
    drain_insert_history(&mut rx);
    chat.on_interrupted_turn(TurnAbortReason::Interrupted);
    drain_insert_history(&mut rx);

    chat.on_task_started();
    drain_insert_history(&mut rx);
    chat.on_interrupted_turn(TurnAbortReason::Interrupted);
    drain_insert_history(&mut rx);

    // ACP backend suppresses both stale Completeds.

    // Start the real turn
    chat.on_task_started();
    drain_insert_history(&mut rx);

    // Tool events for the real turn should work
    begin_exec(&mut chat, "real-call", "echo real");
    assert!(
        chat.active_cell.is_some(),
        "ExecCell should be created - counter was reset by on_task_started"
    );

    // Real Completed should finalize the turn
    chat.on_task_complete(None);
    drain_insert_history(&mut rx);

    assert!(
        !chat.bottom_pane.is_task_running(),
        "Task should be stopped after real Completed following multiple interrupts"
    );
}

#[test]
fn completed_last_agent_message_renders_without_streaming_deltas() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.on_task_started();
    drain_insert_history(&mut rx);

    chat.on_task_complete(Some("RESPONSE_ONE_UNIQUE_MARKER".to_string()));

    let combined = drain_insert_history(&mut rx)
        .into_iter()
        .map(|lines| lines_to_single_string(&lines))
        .collect::<String>();

    assert!(
        combined.contains("RESPONSE_ONE_UNIQUE_MARKER"),
        "completion fallback should render the final assistant message: {combined}"
    );
}

#[test]
fn late_answer_delta_after_completed_fallback_is_ignored() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.on_task_started();
    drain_insert_history(&mut rx);

    chat.on_task_complete(Some("RESPONSE_ONE_UNIQUE_MARKER".to_string()));
    let initial = drain_insert_history(&mut rx)
        .into_iter()
        .map(|lines| lines_to_single_string(&lines))
        .collect::<String>();

    chat.handle_streaming_delta("RESPONSE_ONE_UNIQUE_MARKER".to_string());
    let after_late_delta = drain_insert_history(&mut rx)
        .into_iter()
        .map(|lines| lines_to_single_string(&lines))
        .collect::<String>();

    assert!(
        initial.contains("RESPONSE_ONE_UNIQUE_MARKER"),
        "completion fallback should render once: {initial}"
    );
    assert!(
        after_late_delta.is_empty(),
        "late answer deltas should be ignored after completion fallback: {after_late_delta}"
    );
}

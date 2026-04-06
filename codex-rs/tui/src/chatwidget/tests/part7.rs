use super::*;

/// After an interrupt, the ACP backend's monotonic turn counter guarantees
/// that the stale Completed from the cancelled task is never emitted. The
/// TUI should handle the normal sequence without issues.
///
/// Sequence:
///   1. Started(A)    → task running
///   2. Aborted(A)    → task stopped (user pressed ESC)
///   3. Started(B)    → new turn begins
///   4. Completed(B)  → should finalize turn B normally
#[test]
fn interrupt_then_new_turn_completes_normally() {
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

/// Multiple consecutive interrupts followed by a real turn. The ACP backend's
/// monotonic turn counter suppresses all stale Completeds, so the final real
/// turn's Completed must still finalize normally.
#[test]
fn multiple_interrupts_then_real_turn_completes_normally() {
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
        "ExecCell should be created during real turn"
    );

    // Real Completed should finalize the turn
    chat.on_task_complete(None);
    drain_insert_history(&mut rx);

    assert!(
        !chat.bottom_pane.is_task_running(),
        "Task should be stopped after real Completed following multiple interrupts"
    );
}

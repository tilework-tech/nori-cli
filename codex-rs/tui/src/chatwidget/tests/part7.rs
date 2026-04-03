use super::*;

/// When the stale Completed arrives during an active turn, tool events for the
/// active turn should NOT be discarded.
///
/// Race condition sequence:
///   1. Started(A)    → task running
///   2. Aborted(A)    → task stopped (user pressed ESC)
///   3. Started(B)    → new task running (user submitted new message)
///   4. Completed(A)  → stale event from cancelled background task
///
/// After step 4, turn B's tool events must still be processed. Before the fix,
/// the stale Completed prematurely gated tool events, causing them to be
/// silently discarded.
#[test]
fn stale_completed_should_not_block_tool_events_for_next_turn() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    // Start and interrupt turn A
    chat.on_task_started();
    drain_insert_history(&mut rx);
    chat.on_interrupted_turn(TurnAbortReason::Interrupted);
    drain_insert_history(&mut rx);

    // Start turn B
    chat.on_task_started();
    drain_insert_history(&mut rx);

    // Stale Completed from turn A
    chat.on_task_complete(None);
    drain_insert_history(&mut rx);

    // Tool event for turn B should NOT be discarded
    begin_exec(&mut chat, "turn-b-call", "echo hello from turn B");

    assert!(
        chat.active_cell.is_some(),
        "ExecCell should be created - stale Completed should not block turn B's tool events"
    );
}

/// Multiple consecutive interrupts should each produce one stale Completed
/// that is correctly drained before the real turn's events arrive.
#[test]
fn multiple_interrupts_drain_stale_completes_in_order() {
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

    // Start the real turn
    chat.on_task_started();
    drain_insert_history(&mut rx);

    // Two stale Completeds arrive
    chat.on_task_complete(None);
    chat.on_task_complete(None);

    // Real tool events should still work
    begin_exec(&mut chat, "real-call", "echo real");
    assert!(
        chat.active_cell.is_some(),
        "ExecCell should be created after draining multiple stale Completeds"
    );
}

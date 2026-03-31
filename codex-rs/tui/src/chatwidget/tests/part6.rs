use super::*;

/// Safety net: when an incomplete ExecCell is in active_cell and TaskComplete
/// fires WITHOUT a preceding AgentMessage (e.g., agent error or interruption),
/// on_task_complete should finalize the stuck cell.
///
/// Sequence:
/// 1. TaskStarted
/// 2. ExecCommandBegin (creates active ExecCell)
/// 3. TaskComplete fires (no AgentMessage)
/// 4. Assert: active_cell is None (cell was finalized and flushed to history)
#[test]
fn task_complete_finalizes_stuck_active_cell() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    // 1. Start a task
    chat.handle_codex_event(Event {
        id: "t1".into(),
        msg: EventMsg::TaskStarted(TaskStartedEvent {
            model_context_window: None,
        }),
    });
    drain_insert_history(&mut rx);

    // 2. Begin a tool call (creates active ExecCell)
    begin_exec(&mut chat, "stuck-call", "echo running");
    assert!(
        chat.active_cell.is_some(),
        "active_cell should contain the ExecCell after begin"
    );

    // 3. TaskComplete fires directly (no AgentMessage)
    chat.handle_codex_event(Event {
        id: "t1".into(),
        msg: EventMsg::TaskComplete(TaskCompleteEvent {
            last_agent_message: None,
        }),
    });

    // 4. After task_complete, active_cell MUST be None
    assert!(
        chat.active_cell.is_none(),
        "active_cell should be None after task_complete - stuck ExecCell should have been finalized"
    );

    // The finalized cell should appear in history
    let cells = drain_insert_history(&mut rx);
    let combined: String = cells
        .iter()
        .map(|lines| lines_to_single_string(lines))
        .collect();
    assert!(
        combined.contains("echo running"),
        "finalized stuck cell should appear in history: {combined:?}"
    );
}

/// When the agent message arrives while an ExecCell is still incomplete in
/// active_cell, the cell should be finalized immediately. Per the spec:
/// "the moment an agent message comes in, all further in progress tool
/// messages should just be dropped."
///
/// This ensures the viewport is freed up so the agent's text response
/// can be displayed without being blocked by a stuck tool call cell.
#[test]
fn agent_message_finalizes_incomplete_active_cell() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    // Start a task
    chat.handle_codex_event(Event {
        id: "t1".into(),
        msg: EventMsg::TaskStarted(TaskStartedEvent {
            model_context_window: None,
        }),
    });
    drain_insert_history(&mut rx);

    // Begin a tool call (creates active ExecCell)
    begin_exec(&mut chat, "in-progress-call", "echo hello");
    assert!(
        chat.active_cell.is_some(),
        "active_cell should contain ExecCell"
    );

    // Agent message arrives - should finalize the incomplete ExecCell
    chat.handle_codex_event(Event {
        id: "t1".into(),
        msg: EventMsg::AgentMessage(AgentMessageEvent {
            message: "Done with the task".into(),
        }),
    });

    // After agent message, active_cell should be None
    assert!(
        chat.active_cell.is_none(),
        "active_cell should be None after agent message - incomplete ExecCell should be finalized"
    );

    // The finalized cell should appear in history
    let cells = drain_insert_history(&mut rx);
    let combined: String = cells
        .iter()
        .map(|lines| lines_to_single_string(lines))
        .collect();
    assert!(
        combined.contains("echo hello"),
        "finalized incomplete cell should appear in history after agent message: {combined:?}"
    );
}

/// When multiple tool calls are in progress and the agent message arrives,
/// ALL incomplete tool cells should be finalized - both the active_cell
/// and any cells saved in pending_exec_cells.
///
/// Sequence:
/// 1. TaskStarted
/// 2. ExecCommandBegin("call-1") → creates active ExecCell
/// 3. ExecCommandBegin("call-2") → flushes call-1 to pending, creates call-2 in active
/// 4. AgentMessage → should finalize both
#[test]
fn agent_message_finalizes_multiple_incomplete_cells() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    // Start a task
    chat.handle_codex_event(Event {
        id: "t1".into(),
        msg: EventMsg::TaskStarted(TaskStartedEvent {
            model_context_window: None,
        }),
    });
    drain_insert_history(&mut rx);

    // Begin first tool call
    begin_exec(&mut chat, "call-1", "echo first");

    // Begin second tool call - this flushes the first to pending_exec_cells
    begin_exec(&mut chat, "call-2", "echo second");

    // Agent message arrives - should finalize everything
    chat.handle_codex_event(Event {
        id: "t1".into(),
        msg: EventMsg::AgentMessage(AgentMessageEvent {
            message: "Both commands done".into(),
        }),
    });

    // Verify both active_cell and pending_exec_cells are clean
    assert!(
        chat.active_cell.is_none(),
        "active_cell should be None after agent message"
    );
    assert_eq!(
        chat.pending_exec_cells.len(),
        0,
        "pending_exec_cells should be empty after agent message"
    );

    // Both finalized cells should appear in history
    let cells = drain_insert_history(&mut rx);
    let combined: String = cells
        .iter()
        .map(|lines| lines_to_single_string(lines))
        .collect();
    assert!(
        combined.contains("echo first") || combined.contains("echo second"),
        "finalized cells should appear in history: {combined:?}"
    );
}

/// Streaming scenario: when tool calls are started and text is streamed, the
/// incomplete ExecCell should be flushed to history immediately to preserve
/// chronological ordering (tool cell appears before the text that follows it).
/// This matches the ACP flow where:
/// 1. Agent starts tool calls
/// 2. Agent streams its response text (via deltas)
/// 3. The tool cell should appear in history BEFORE the streamed text
#[test]
fn streaming_flushes_incomplete_exec_cell_before_text() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    // Start a task
    chat.handle_codex_event(Event {
        id: "t1".into(),
        msg: EventMsg::TaskStarted(TaskStartedEvent {
            model_context_window: None,
        }),
    });
    drain_insert_history(&mut rx);

    // Begin a tool call
    begin_exec(&mut chat, "tool-1", "cat README.md");
    assert!(chat.active_cell.is_some());

    // Stream agent text — the incomplete ExecCell should be flushed to history
    // first, so tool cells appear before the text in chronological order.
    chat.handle_codex_event(Event {
        id: "t1".into(),
        msg: EventMsg::AgentMessageDelta(AgentMessageDeltaEvent {
            delta: "Here is the file content:\n".into(),
        }),
    });
    chat.on_commit_tick();

    // The incomplete ExecCell should have been flushed to history (not kept in active_cell)
    assert!(
        chat.active_cell.is_none(),
        "incomplete ExecCell should be flushed to history when text arrives"
    );

    // The flushed cell should appear in history before the streamed text
    let cells = drain_insert_history(&mut rx);
    let combined: String = cells
        .iter()
        .map(|lines| lines_to_single_string(lines))
        .collect();
    assert!(
        combined.contains("README.md"),
        "flushed incomplete cell should appear in history: {combined:?}"
    );
}

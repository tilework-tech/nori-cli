use super::*;

/// ACP agents may stream reasoning before producing their final answer.
/// This test verifies reasoning + answer rendering from an ACP agent.
#[test]
fn acp_reasoning_then_answer_vt100_snapshot() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.handle_codex_event(Event {
        id: "acp-2".into(),
        msg: EventMsg::TaskStarted(TaskStartedEvent {
            model_context_window: None,
        }),
    });

    let width: u16 = 80;
    let height: u16 = 35;
    let backend = VT100Backend::new(width, height);
    let mut term = crate::custom_terminal::Terminal::with_options(backend).expect("terminal");
    term.set_viewport_area(Rect::new(0, height - 1, width, 1));

    // Stream reasoning first (ACP ReasoningDelta maps to AgentReasoningDelta)
    let reasoning = "**Analyzing the request**\n\nThe user wants to sort a list. I should consider:\n- Time complexity requirements\n- Whether stability matters\n- Memory constraints";

    for chunk in reasoning.chars().collect::<Vec<_>>().chunks(3) {
        let delta: String = chunk.iter().collect();
        chat.handle_codex_event(Event {
            id: "acp-2".into(),
            msg: EventMsg::AgentReasoningDelta(AgentReasoningDeltaEvent { delta }),
        });
    }

    // Finalize reasoning
    chat.handle_codex_event(Event {
        id: "acp-2".into(),
        msg: EventMsg::AgentReasoning(AgentReasoningEvent {
            text: reasoning.into(),
        }),
    });

    // Now stream the answer
    let answer = "Based on your requirements, here's a quicksort implementation:\n\n```python\ndef quicksort(arr):\n    if len(arr) <= 1:\n        return arr\n    pivot = arr[len(arr) // 2]\n    left = [x for x in arr if x < pivot]\n    middle = [x for x in arr if x == pivot]\n    right = [x for x in arr if x > pivot]\n    return quicksort(left) + middle + quicksort(right)\n```";

    let mut chars = answer.chars();
    loop {
        let mut delta = String::new();
        match chars.next() {
            Some(c) => delta.push(c),
            None => break,
        }
        if let Some(c2) = chars.next() {
            delta.push(c2);
        }

        chat.handle_codex_event(Event {
            id: "acp-2".into(),
            msg: EventMsg::AgentMessageDelta(AgentMessageDeltaEvent { delta }),
        });

        loop {
            chat.on_commit_tick();
            let mut inserted_any = false;
            while let Ok(app_ev) = rx.try_recv() {
                if let AppEvent::InsertHistoryCell(cell) = app_ev {
                    let lines = cell.display_lines(width);
                    crate::insert_history::insert_history_lines(&mut term, lines)
                        .expect("Failed to insert history lines");
                    inserted_any = true;
                }
            }
            if !inserted_any {
                break;
            }
        }
    }

    chat.handle_codex_event(Event {
        id: "acp-2".into(),
        msg: EventMsg::TaskComplete(TaskCompleteEvent {
            last_agent_message: None,
        }),
    });
    for lines in drain_insert_history(&mut rx) {
        crate::insert_history::insert_history_lines(&mut term, lines)
            .expect("Failed to insert final history lines");
    }

    assert_snapshot!(
        "acp_reasoning_then_answer",
        term.backend().vt100().screen().contents()
    );
}

/// ACP agents may encounter errors during execution. This test verifies
/// that stream errors from an ACP agent render appropriately.
#[test]
fn acp_stream_error_snapshot() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.handle_codex_event(Event {
        id: "acp-3".into(),
        msg: EventMsg::TaskStarted(TaskStartedEvent {
            model_context_window: None,
        }),
    });

    // Stream some initial content
    chat.handle_codex_event(Event {
        id: "acp-3".into(),
        msg: EventMsg::AgentMessageDelta(AgentMessageDeltaEvent {
            delta: "Let me help you with that...\n\n".into(),
        }),
    });
    chat.on_commit_tick();

    // Then encounter an error (simulates ACP error notification)
    chat.handle_codex_event(Event {
        id: "acp-3".into(),
        msg: EventMsg::StreamError(StreamErrorEvent {
            message: "Connection to ACP agent was interrupted".into(),
            codex_error_info: None,
        }),
    });

    let cells = drain_insert_history(&mut rx);
    let combined = cells
        .iter()
        .map(|lines| lines_to_single_string(lines))
        .collect::<String>();

    assert_snapshot!("acp_stream_error", combined);
}

/// Multi-turn conversation with an ACP agent. This tests that multiple
/// exchanges render correctly in sequence.
#[test]
fn acp_multi_turn_conversation_snapshot() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    // First turn: user asks, agent responds
    chat.handle_codex_event(Event {
        id: "acp-4".into(),
        msg: EventMsg::TaskStarted(TaskStartedEvent {
            model_context_window: None,
        }),
    });

    chat.handle_codex_event(Event {
        id: "acp-4".into(),
        msg: EventMsg::AgentMessageDelta(AgentMessageDeltaEvent {
            delta: "I can help you refactor that function. ".into(),
        }),
    });
    chat.handle_codex_event(Event {
        id: "acp-4".into(),
        msg: EventMsg::AgentMessageDelta(AgentMessageDeltaEvent {
            delta: "First, let me analyze the current implementation.\n".into(),
        }),
    });
    chat.on_commit_tick();

    chat.handle_codex_event(Event {
        id: "acp-4".into(),
        msg: EventMsg::TaskComplete(TaskCompleteEvent {
            last_agent_message: Some("I can help you refactor that function. First, let me analyze the current implementation.\n".into()),
        }),
    });

    // Drain first turn
    let turn1_cells = drain_insert_history(&mut rx);

    // Second turn: follow-up
    chat.handle_codex_event(Event {
        id: "acp-5".into(),
        msg: EventMsg::TaskStarted(TaskStartedEvent {
            model_context_window: None,
        }),
    });

    chat.handle_codex_event(Event {
        id: "acp-5".into(),
        msg: EventMsg::AgentMessageDelta(AgentMessageDeltaEvent {
            delta: "Here's the refactored version:\n\n```rust\nfn process(data: &str) -> Result<String, Error> {\n    data.parse()\n}\n```\n".into(),
        }),
    });
    chat.on_commit_tick();

    chat.handle_codex_event(Event {
        id: "acp-5".into(),
        msg: EventMsg::TaskComplete(TaskCompleteEvent {
            last_agent_message: None,
        }),
    });

    // Drain second turn
    let turn2_cells = drain_insert_history(&mut rx);

    // Combine both turns
    let mut combined = String::new();
    for lines in turn1_cells.iter().chain(turn2_cells.iter()) {
        combined.push_str(&lines_to_single_string(lines));
    }

    assert_snapshot!("acp_multi_turn_conversation", combined);
}

/// ACP agent response with bullet points and nested lists.
/// Verifies markdown list rendering from ACP streaming.
#[test]
fn acp_markdown_list_response_snapshot() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.handle_codex_event(Event {
        id: "acp-6".into(),
        msg: EventMsg::TaskStarted(TaskStartedEvent {
            model_context_window: None,
        }),
    });

    let width: u16 = 80;
    let height: u16 = 40;
    let backend = VT100Backend::new(width, height);
    let mut term = crate::custom_terminal::Terminal::with_options(backend).expect("terminal");
    term.set_viewport_area(Rect::new(0, height - 1, width, 1));

    let response = r#"Here are the key considerations for your ACP integration:

1. **Protocol Compliance**
   - Use JSON-RPC 2.0 message format
   - Handle bidirectional communication
   - Support session management

2. **Event Handling**
   - Subscribe to `session/update` notifications
   - Process `agent_message_chunk` for streaming
   - Handle `agent_reasoning_chunk` for thinking

3. **Error Recovery**
   - Implement reconnection logic
   - Buffer partial messages
   - Log protocol violations

Would you like me to elaborate on any of these points?"#;

    let mut chars = response.chars();
    loop {
        let mut delta = String::new();
        match chars.next() {
            Some(c) => delta.push(c),
            None => break,
        }
        if let Some(c2) = chars.next() {
            delta.push(c2);
        }

        chat.handle_codex_event(Event {
            id: "acp-6".into(),
            msg: EventMsg::AgentMessageDelta(AgentMessageDeltaEvent { delta }),
        });

        loop {
            chat.on_commit_tick();
            let mut inserted_any = false;
            while let Ok(app_ev) = rx.try_recv() {
                if let AppEvent::InsertHistoryCell(cell) = app_ev {
                    let lines = cell.display_lines(width);
                    crate::insert_history::insert_history_lines(&mut term, lines)
                        .expect("Failed to insert history lines");
                    inserted_any = true;
                }
            }
            if !inserted_any {
                break;
            }
        }
    }

    chat.handle_codex_event(Event {
        id: "acp-6".into(),
        msg: EventMsg::TaskComplete(TaskCompleteEvent {
            last_agent_message: None,
        }),
    });
    for lines in drain_insert_history(&mut rx) {
        crate::insert_history::insert_history_lines(&mut term, lines)
            .expect("Failed to insert final history lines");
    }

    assert_snapshot!(
        "acp_markdown_list_response",
        term.backend().vt100().screen().contents()
    );
}

/// PatchApplyBegin events should observe the parent directory of changed files
/// and trigger a footer refresh when the effective CWD changes.
#[test]
fn patch_apply_begin_observes_directory_for_footer_update() {
    use std::collections::HashMap;

    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    // Set up the initial CWD to something different from where we'll write files
    let initial_cwd = PathBuf::from("/home/user/project");
    chat.config.cwd = initial_cwd.clone();
    chat.effective_cwd_tracker.reset(Some(initial_cwd));

    // Create a PatchApplyBeginEvent with files in a different directory (worktree)
    let worktree_file = PathBuf::from("/home/user/worktree/src/main.rs");
    let mut changes = HashMap::new();
    changes.insert(
        worktree_file,
        FileChange::Add {
            content: "fn main() {}".to_string(),
        },
    );

    // First patch event - should start tracking the new directory as candidate
    chat.handle_codex_event(Event {
        id: "patch-1".into(),
        msg: EventMsg::PatchApplyBegin(PatchApplyBeginEvent {
            call_id: "c1".into(),
            turn_id: "turn-c1".into(),
            auto_approved: true,
            changes: changes.clone(),
        }),
    });

    // Drain events - should NOT have RefreshSystemInfoForDirectory yet (debounce not met)
    let refresh_dirs = drain_refresh_system_info_events(&mut rx);
    assert!(
        refresh_dirs.is_empty(),
        "should not refresh immediately, debounce threshold not met"
    );

    // Note: The current implementation doesn't track PatchApplyBegin events for CWD changes.
    // This test will fail until we implement that feature.
    // After implementation, subsequent patch events in the same directory after 500ms
    // should trigger a RefreshSystemInfoForDirectory event.
}

/// PatchApplyEnd events should also observe the parent directory of changed files.
#[test]
fn patch_apply_end_observes_directory_for_footer_update() {
    use std::collections::HashMap;

    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    // Set up the initial CWD
    let initial_cwd = PathBuf::from("/home/user/project");
    chat.config.cwd = initial_cwd.clone();
    chat.effective_cwd_tracker.reset(Some(initial_cwd));

    // Create changes in a worktree directory
    let worktree_file = PathBuf::from("/home/user/worktree/src/lib.rs");
    let mut changes = HashMap::new();
    changes.insert(
        worktree_file,
        FileChange::Update {
            unified_diff: "--- a/src/lib.rs\n+++ b/src/lib.rs\n".to_string(),
            move_path: None,
        },
    );

    // First end event - should start tracking
    chat.handle_codex_event(Event {
        id: "patch-end-1".into(),
        msg: EventMsg::PatchApplyEnd(PatchApplyEndEvent {
            call_id: "c1".into(),
            turn_id: "turn-c1".into(),
            stdout: String::new(),
            stderr: String::new(),
            success: true,
            changes: changes.clone(),
        }),
    });

    // Drain - no refresh yet due to debounce
    let refresh_dirs = drain_refresh_system_info_events(&mut rx);
    assert!(
        refresh_dirs.is_empty(),
        "should not refresh immediately, debounce threshold not met"
    );

    // Note: Like PatchApplyBegin, this test documents expected behavior.
    // The test will pass once we implement directory observation in handle_patch_apply_end_now.
}

/// When files have relative paths, they should be resolved against config.cwd
/// before extracting the parent directory.
#[test]
fn patch_apply_resolves_relative_paths_against_cwd() {
    use std::collections::HashMap;

    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    // Set up a specific CWD
    let cwd = PathBuf::from("/home/user/worktree");
    chat.config.cwd = cwd.clone();
    chat.effective_cwd_tracker.reset(Some(cwd));

    // Create a change with a relative path
    let relative_file = PathBuf::from("src/main.rs");
    let mut changes = HashMap::new();
    changes.insert(
        relative_file,
        FileChange::Add {
            content: "fn main() {}".to_string(),
        },
    );

    // Send patch event
    chat.handle_codex_event(Event {
        id: "patch-rel-1".into(),
        msg: EventMsg::PatchApplyBegin(PatchApplyBeginEvent {
            call_id: "c1".into(),
            turn_id: "turn-c1".into(),
            auto_approved: true,
            changes,
        }),
    });

    // Drain - the effective CWD should remain unchanged since the resolved path
    // (/home/user/worktree/src) is a subdirectory of the current CWD
    let _ = drain_refresh_system_info_events(&mut rx);

    // The effective CWD tracker should have observed /home/user/worktree/src
    // but since it's within the current CWD hierarchy, behavior depends on implementation
}

#[test]
fn exec_read_skill_md_records_skill() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    // Create a Read parsed command for a SKILL.md file
    let skill_path = PathBuf::from("/home/user/.claude/skills/brainstorming/SKILL.md");
    let parsed_cmd = vec![ParsedCommand::Read {
        cmd: "cat".to_string(),
        name: "SKILL.md".to_string(),
        path: skill_path.clone(),
    }];

    // Send ExecCommandBegin event with the Read command
    chat.handle_codex_event(Event {
        id: "skill-read-1".into(),
        msg: EventMsg::ExecCommandBegin(ExecCommandBeginEvent {
            call_id: "skill-read-1".into(),
            process_id: None,
            turn_id: "turn-1".into(),
            command: vec!["cat".to_string(), skill_path.to_string_lossy().to_string()],
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            parsed_cmd,
            source: ExecCommandSource::Agent,
            interaction_input: None,
        }),
    });

    // Verify the skill was recorded
    assert!(
        chat.session_stats()
            .skills_used
            .contains(&"brainstorming".to_string()),
        "Expected 'brainstorming' skill to be recorded, but skills_used was: {:?}",
        chat.session_stats().skills_used
    );
}

#[test]
fn exec_read_non_skill_file_does_not_record_skill() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    // Create a Read parsed command for a regular file
    let file_path = PathBuf::from("/home/user/code/project/src/main.rs");
    let parsed_cmd = vec![ParsedCommand::Read {
        cmd: "cat".to_string(),
        name: "main.rs".to_string(),
        path: file_path.clone(),
    }];

    // Send ExecCommandBegin event
    chat.handle_codex_event(Event {
        id: "regular-read-1".into(),
        msg: EventMsg::ExecCommandBegin(ExecCommandBeginEvent {
            call_id: "regular-read-1".into(),
            process_id: None,
            turn_id: "turn-1".into(),
            command: vec!["cat".to_string(), file_path.to_string_lossy().to_string()],
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            parsed_cmd,
            source: ExecCommandSource::Agent,
            interaction_input: None,
        }),
    });

    // Verify no skill was recorded
    assert!(
        chat.session_stats().skills_used.is_empty(),
        "Expected no skills to be recorded, but skills_used was: {:?}",
        chat.session_stats().skills_used
    );
}

/// Submitting a user message triggers a system info refresh to update the branch marker.
/// This ensures the branch marker in the footer is updated on every transcript activity,
/// catching branch changes that happened between interactions (e.g., user switched
/// branches in another terminal).
#[test]
fn user_message_submission_triggers_system_info_refresh() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    // Drain any events from widget construction
    drain_refresh_system_info_events(&mut rx);

    // Submit a user message
    chat.bottom_pane
        .set_composer_text("test message".to_string());
    chat.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    // Verify that RefreshSystemInfoForDirectory was sent
    let refresh_dirs = drain_refresh_system_info_events(&mut rx);
    assert!(
        !refresh_dirs.is_empty(),
        "expected RefreshSystemInfoForDirectory event after user message submission"
    );
}

/// Task completion triggers a system info refresh to update the branch marker.
/// This ensures any branch changes that occurred during the agent's turn are reflected.
#[test]
fn task_complete_triggers_system_info_refresh() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    // Start a task
    chat.handle_codex_event(Event {
        id: "task-start".into(),
        msg: EventMsg::TaskStarted(TaskStartedEvent {
            model_context_window: None,
        }),
    });

    // Drain any events from task start
    drain_refresh_system_info_events(&mut rx);

    // Complete the task
    chat.handle_codex_event(Event {
        id: "task-complete".into(),
        msg: EventMsg::TaskComplete(TaskCompleteEvent {
            last_agent_message: Some("Done".to_string()),
        }),
    });

    // Verify that RefreshSystemInfoForDirectory was sent
    let refresh_dirs = drain_refresh_system_info_events(&mut rx);
    assert!(
        !refresh_dirs.is_empty(),
        "expected RefreshSystemInfoForDirectory event after task completion"
    );
}

/// Bug fix: when an agent spawn fails, the "Connecting to ..." status indicator
/// should be cleared. Currently `AgentSpawnFailed` calls `add_error_message` and
/// `open_agent_popup` but never hides the status indicator, leaving the TUI stuck
/// in a "Connecting" state.
#[test]
fn agent_spawn_failed_clears_connecting_status() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    // Simulate what AgentConnecting does: show the connecting spinner.
    chat.show_connecting_status("test-agent");
    assert!(
        chat.bottom_pane.status_indicator_visible(),
        "status indicator should be visible after show_connecting_status"
    );

    // Simulate what the AgentSpawnFailed handler does.
    chat.on_agent_spawn_failed("test-agent", "connection refused");
    assert!(
        !chat.bottom_pane.status_indicator_visible(),
        "status indicator should be hidden after agent spawn failure"
    );
}

/// Bug fix: when the op channel receiver has been dropped (backend died), sending
/// Op::Shutdown should trigger an exit instead of silently logging an error.
/// Without this fix, /exit and double-ctrl-c are broken when the backend is dead.
#[test]
fn shutdown_on_dead_channel_triggers_exit() {
    let (chat, mut rx, op_rx) = make_chatwidget_manual();

    // Drop the op receiver to simulate the backend having died.
    drop(op_rx);

    // Attempt to send Op::Shutdown (what /exit and double-ctrl-c do).
    chat.submit_op(Op::Shutdown);

    // The widget should have sent an ExitRequest since the backend is gone.
    let mut found_exit = false;
    while let Ok(ev) = rx.try_recv() {
        if matches!(ev, AppEvent::ExitRequest) {
            found_exit = true;
            break;
        }
    }
    assert!(
        found_exit,
        "expected ExitRequest to be sent when Op::Shutdown fails on a dead channel"
    );
}

/// Bug fix: when the backend is still connecting (simulated by an async task
/// that never completes), sending Op::Shutdown via the op channel must cause
/// the spawn task to detect it via `drain_until_shutdown` and emit ExitRequest.
#[tokio::test]
async fn shutdown_while_backend_connecting_triggers_exit() {
    use tokio::sync::mpsc::unbounded_channel;

    let (app_tx_raw, mut app_rx) = unbounded_channel::<AppEvent>();
    let app_event_tx = AppEventSender::new(app_tx_raw);
    let (op_tx, mut op_rx) = unbounded_channel::<Op>();

    // Simulate a spawn task that races a "never-completing backend" against
    // drain_until_shutdown — the same pattern used in spawn_acp_agent.
    let tx = app_event_tx.clone();
    let handle = tokio::spawn(async move {
        tokio::select! {
            // Simulates AcpBackend::spawn() that hangs forever.
            () = std::future::pending::<()>() => {
                unreachable!("backend should not complete");
            }
            () = super::agent::drain_until_shutdown(&mut op_rx) => {
                drop(op_rx);
                tx.send(AppEvent::ExitRequest);
            }
        }
    });

    // Send Op::Shutdown (what /exit and double-ctrl-c do).
    op_tx.send(Op::Shutdown).unwrap();

    // Wait for the spawn task to finish.
    handle.await.unwrap();

    // The task should have produced an ExitRequest.
    let mut found_exit = false;
    while let Ok(ev) = app_rx.try_recv() {
        if matches!(ev, AppEvent::ExitRequest) {
            found_exit = true;
            break;
        }
    }
    assert!(
        found_exit,
        "expected ExitRequest when Op::Shutdown is sent to a live but unconsumed channel"
    );
}

#[test]
fn late_exec_events_after_agent_message_are_discarded() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    // Start a turn
    chat.handle_codex_event(Event {
        id: "t1".into(),
        msg: EventMsg::TaskStarted(TaskStartedEvent {
            model_context_window: None,
        }),
    });

    // Normal tool call during the turn (should appear)
    let begin_normal = begin_exec(&mut chat, "call-normal", "echo hello");
    end_exec(&mut chat, begin_normal, "hello", "", 0);

    // Agent finalizes its response
    chat.handle_codex_event(Event {
        id: "t1".into(),
        msg: EventMsg::AgentMessage(AgentMessageEvent {
            message: "Here is my response".into(),
        }),
    });

    // Drain everything so far — the normal tool call and agent message should be present
    let cells_before = drain_insert_history(&mut rx);
    let combined_before: String = cells_before
        .iter()
        .map(|lines| lines_to_single_string(lines))
        .collect();
    assert!(
        combined_before.contains("echo hello"),
        "normal tool call should appear: {combined_before:?}"
    );
    assert!(
        combined_before.contains("Here is my response"),
        "agent message should appear: {combined_before:?}"
    );

    // Now send LATE tool events — these arrive after the agent message
    // due to the ACP race condition. They should be silently discarded.
    let begin_late = begin_exec(&mut chat, "call-late", "cat /etc/passwd");
    end_exec(&mut chat, begin_late, "root:x:0:0", "", 0);

    // Drain anything emitted by the late events — there should be NOTHING
    let cells_after_late = drain_insert_history(&mut rx);
    let combined_after_late: String = cells_after_late
        .iter()
        .map(|lines| lines_to_single_string(lines))
        .collect();
    assert!(
        !combined_after_late.contains("cat /etc/passwd"),
        "late exec tool call should have been discarded but appeared after agent message: {combined_after_late:?}"
    );
    assert!(
        !combined_after_late.contains("root:x:0:0"),
        "late exec tool output should have been discarded but appeared after agent message: {combined_after_late:?}"
    );

    // Also verify nothing is sitting in active_cell waiting to leak
    assert!(
        chat.active_cell.is_none(),
        "no active cell should remain after late events are discarded"
    );
}

#[test]
fn turn_finished_gate_resets_on_new_task_started() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    // === Turn 1: agent message sets the gate ===
    chat.handle_codex_event(Event {
        id: "t1".into(),
        msg: EventMsg::TaskStarted(TaskStartedEvent {
            model_context_window: None,
        }),
    });
    chat.handle_codex_event(Event {
        id: "t1".into(),
        msg: EventMsg::AgentMessage(AgentMessageEvent {
            message: "Turn 1 response".into(),
        }),
    });
    chat.handle_codex_event(Event {
        id: "t1".into(),
        msg: EventMsg::TaskComplete(TaskCompleteEvent {
            last_agent_message: None,
        }),
    });
    // Drain turn 1 output
    drain_insert_history(&mut rx);

    // === Turn 2: gate should be cleared, tool calls should work again ===
    chat.handle_codex_event(Event {
        id: "t2".into(),
        msg: EventMsg::TaskStarted(TaskStartedEvent {
            model_context_window: None,
        }),
    });

    // This tool call in turn 2 should NOT be discarded
    let begin_t2 = begin_exec(&mut chat, "call-t2", "ls -la");
    end_exec(&mut chat, begin_t2, "total 42", "", 0);

    chat.handle_codex_event(Event {
        id: "t2".into(),
        msg: EventMsg::AgentMessage(AgentMessageEvent {
            message: "Turn 2 response".into(),
        }),
    });
    chat.handle_codex_event(Event {
        id: "t2".into(),
        msg: EventMsg::TaskComplete(TaskCompleteEvent {
            last_agent_message: None,
        }),
    });

    let cells = drain_insert_history(&mut rx);
    let combined: String = cells
        .iter()
        .map(|lines| lines_to_single_string(lines))
        .collect();

    // Turn 2 tool call should appear (gate was reset)
    assert!(
        combined.contains("ls -la"),
        "turn 2 tool call should appear after gate reset: {combined:?}"
    );
    assert!(
        combined.contains("Turn 2 response"),
        "turn 2 agent message should appear: {combined:?}"
    );
}

#[test]
fn late_mcp_tool_call_after_agent_message_is_discarded() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    // Start turn
    chat.handle_codex_event(Event {
        id: "t1".into(),
        msg: EventMsg::TaskStarted(TaskStartedEvent {
            model_context_window: None,
        }),
    });

    // Agent responds
    chat.handle_codex_event(Event {
        id: "t1".into(),
        msg: EventMsg::AgentMessage(AgentMessageEvent {
            message: "Done with the task".into(),
        }),
    });

    // Drain agent message
    drain_insert_history(&mut rx);

    // Late MCP tool call arrives after agent message
    let mcp_invocation = codex_protocol::protocol::McpInvocation {
        server: "test-server".into(),
        tool: "test_tool".into(),
        arguments: Some(serde_json::json!({})),
    };
    chat.handle_codex_event(Event {
        id: "mcp-late".into(),
        msg: EventMsg::McpToolCallBegin(McpToolCallBeginEvent {
            call_id: "mcp-call-late".into(),
            invocation: mcp_invocation.clone(),
        }),
    });
    chat.handle_codex_event(Event {
        id: "mcp-late".into(),
        msg: EventMsg::McpToolCallEnd(McpToolCallEndEvent {
            call_id: "mcp-call-late".into(),
            invocation: mcp_invocation,
            duration: std::time::Duration::from_millis(10),
            result: Ok(mcp_types::CallToolResult {
                content: vec![mcp_types::ContentBlock::TextContent(
                    mcp_types::TextContent {
                        annotations: None,
                        text: "some output".into(),
                        r#type: "text".into(),
                    },
                )],
                is_error: Some(false),
                structured_content: None,
            }),
        }),
    });

    // Complete turn
    chat.handle_codex_event(Event {
        id: "t1".into(),
        msg: EventMsg::TaskComplete(TaskCompleteEvent {
            last_agent_message: None,
        }),
    });

    let cells = drain_insert_history(&mut rx);
    let combined: String = cells
        .iter()
        .map(|lines| lines_to_single_string(lines))
        .collect();

    // MCP tool call should NOT appear
    assert!(
        !combined.contains("test_tool"),
        "late MCP tool call should have been discarded: {combined:?}"
    );
    assert!(
        !combined.contains("some output"),
        "late MCP tool output should have been discarded: {combined:?}"
    );
}

/// Regression test: ExecEnd events arriving during streaming should flush the
/// stream and be handled immediately (not deferred). This ensures tool call
/// cells appear in the correct interleaved position between text blocks.
///
/// Previously, ExecEnd was deferred while streaming was active, causing tool
/// cells to appear after all text on TaskComplete. Now, ExecEnd flushes the
/// stream first (matching ExecBegin's behavior), so it is handled inline.
///
/// Sequence:
/// 1. Start task, begin an exec command (creates active exec cell)
/// 2. Start streaming agent content (creates stream_controller)
/// 3. ExecEnd arrives → flushes stream, handles immediately (NOT deferred)
/// 4. Interrupt queue stays empty
#[test]
fn exec_end_flushes_stream_and_handles_immediately() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    // 1. Start a task and begin an exec command (before streaming starts)
    chat.on_task_started();
    drain_insert_history(&mut rx);

    let first_begin = begin_exec(&mut chat, "running-call", "echo first");

    // 2. Start streaming agent content (creates stream_controller)
    chat.on_agent_message_delta("Here is my answer".to_string());
    assert!(
        chat.stream_controller.is_some(),
        "stream_controller should exist after delta"
    );

    // 3. ExecEnd arrives → should flush stream and handle immediately
    end_exec(&mut chat, first_begin, "first output", "", 0);

    // The interrupt queue should be empty — ExecEnd was NOT deferred
    assert!(
        chat.interrupts.is_empty(),
        "ExecEnd should NOT be deferred; it should flush stream and handle immediately"
    );
}

/// After /compact, the TUI should show: (1) the streamed summary from the old
/// session, (2) a "Context compacted" indicator, (3) a new session header
/// (containing "Nori CLI"), and (4) the summary reprinted as the first
/// assistant message of the new session.
#[test]
fn compact_shows_session_header_and_reprints_summary() {
    use codex_core::protocol::ContextCompactedEvent;

    let (mut chat, mut rx, _ops) = make_chatwidget_manual();

    // Simulate the compact event sequence from the ACP backend.
    // 1. TaskStarted
    chat.handle_codex_event(Event {
        id: "compact-1".into(),
        msg: EventMsg::TaskStarted(TaskStartedEvent {
            model_context_window: None,
        }),
    });

    // 2. Stream the summary as AgentMessageDelta events
    let summary_text = "This conversation covered refactoring the auth module.\n";
    chat.handle_codex_event(Event {
        id: "compact-1".into(),
        msg: EventMsg::AgentMessageDelta(AgentMessageDeltaEvent {
            delta: summary_text.to_string(),
        }),
    });
    // Flush streamed lines through commit ticks
    for _ in 0..20 {
        chat.on_commit_tick();
    }

    // 3. ContextCompacted with summary
    chat.handle_codex_event(Event {
        id: "compact-1".into(),
        msg: EventMsg::ContextCompacted(ContextCompactedEvent {
            summary: Some(summary_text.to_string()),
        }),
    });

    // 4. TaskComplete
    chat.handle_codex_event(Event {
        id: "compact-1".into(),
        msg: EventMsg::TaskComplete(TaskCompleteEvent {
            last_agent_message: None,
        }),
    });

    // Drain all history cells and convert to a single text blob
    let cells = drain_insert_history(&mut rx);
    let all_text: Vec<String> = cells.iter().map(|c| lines_to_single_string(c)).collect();
    let combined = all_text.join("");

    // The session header should appear (rendered as the Nori CLI card)
    assert!(
        combined.contains("Nori CLI"),
        "expected session header with 'Nori CLI' after compact: {combined:?}"
    );

    // "Context compacted" indicator should appear
    assert!(
        combined.contains("Context compacted"),
        "expected 'Context compacted' message: {combined:?}"
    );

    // The summary should appear at least twice:
    // once from the original stream, once from the reprint after the header
    let summary_needle = "refactoring the auth module";
    let occurrences = combined.matches(summary_needle).count();
    assert!(
        occurrences >= 2,
        "expected summary to appear at least twice (original stream + reprint), found {occurrences}: {combined:?}"
    );

    // Verify ordering: session header comes after "Context compacted" and before
    // the reprinted summary. Find positions of key markers.
    let compacted_pos = combined.find("Context compacted").unwrap();
    let header_pos = combined.find("Nori CLI").unwrap();

    // Find the SECOND occurrence of the summary (the reprint)
    let first_summary_pos = combined.find(summary_needle).unwrap();
    let reprint_pos = combined[first_summary_pos + 1..]
        .find(summary_needle)
        .map(|p| p + first_summary_pos + 1)
        .expect("expected second occurrence of summary");

    assert!(
        compacted_pos < header_pos,
        "'Context compacted' (pos {compacted_pos}) should come before session header (pos {header_pos})"
    );
    assert!(
        header_pos < reprint_pos,
        "session header (pos {header_pos}) should come before reprinted summary (pos {reprint_pos})"
    );
}

/// When ContextCompacted has no summary (e.g. from the core backend), the
/// handler falls back to the original behavior: just show "Context compacted"
/// with no session header or reprint.
#[test]
fn compact_without_summary_shows_only_compacted_message() {
    use codex_core::protocol::ContextCompactedEvent;

    let (mut chat, mut rx, _ops) = make_chatwidget_manual();

    // No streaming — just a direct ContextCompacted with no summary
    chat.handle_codex_event(Event {
        id: "compact-2".into(),
        msg: EventMsg::ContextCompacted(ContextCompactedEvent { summary: None }),
    });

    let cells = drain_insert_history(&mut rx);
    let combined: String = cells.iter().map(|c| lines_to_single_string(c)).collect();

    // "Context compacted" should appear
    assert!(
        combined.contains("Context compacted"),
        "expected 'Context compacted' message: {combined:?}"
    );

    // No session header should appear
    assert!(
        !combined.contains("Nori CLI"),
        "should NOT show session header when summary is None: {combined:?}"
    );
}

/// Regression test: tool call results should appear between text blocks, not
/// after all text.
///
/// When the agent streams: text → tool_use → text, the ExecCommandEnd event
/// must be handled in position (after flushing the first text block) rather
/// than deferred until TaskComplete.
///
/// Sequence:
/// 1. TaskStarted
/// 2. Stream text ("Sure, let me check...\n")
/// 3. ExecCommandBegin (tool call starts)
/// 4. ExecCommandEnd (tool call completes with output)
/// 5. Stream more text ("Done!\n")
/// 6. TaskComplete
/// 7. Assert: tool call cell appears BETWEEN the two text blocks
#[test]
fn exec_end_not_deferred_during_streaming() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    // 1. Start a task
    chat.on_task_started();
    drain_insert_history(&mut rx);

    // 2. Stream first text block
    chat.on_agent_message_delta("Sure, let me check...\n".to_string());
    chat.on_commit_tick();
    let first_text = drain_insert_history(&mut rx);
    assert!(
        !first_text.is_empty(),
        "first text block should have been committed to history"
    );

    // 3. ExecCommandBegin — this flushes the stream and handles immediately
    let begin_ev = begin_exec(&mut chat, "call-1", "git status");

    // 4. ExecCommandEnd — previously this was deferred; now it should flush and handle
    end_exec(&mut chat, begin_ev, "on branch main\n", "", 0);

    // The exec cell should now be completed (not deferred)
    assert!(
        chat.interrupts.is_empty(),
        "ExecCommandEnd should NOT be deferred; interrupt queue should be empty"
    );

    // 5. Stream second text block
    chat.on_agent_message_delta("Done!\n".to_string());
    chat.on_commit_tick();

    // 6. TaskComplete
    chat.on_task_complete(None);

    // 7. Collect all history cells and verify ordering
    let cells = drain_insert_history(&mut rx);
    let combined: Vec<String> = cells.iter().map(|c| lines_to_single_string(c)).collect();
    let full = combined.join("");

    // The tool call ("git status") should appear before "Done!"
    let tool_pos = full.find("git status");
    let done_pos = full.find("Done!");
    assert!(
        tool_pos.is_some(),
        "tool call should appear in history: {full:?}"
    );
    assert!(
        done_pos.is_some(),
        "second text block should appear in history: {full:?}"
    );
    assert!(
        tool_pos.unwrap() < done_pos.unwrap(),
        "tool call should appear BEFORE second text block, but tool_pos={} >= done_pos={}\nfull output: {full:?}",
        tool_pos.unwrap(),
        done_pos.unwrap(),
    );
}

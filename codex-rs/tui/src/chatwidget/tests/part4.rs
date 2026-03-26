use super::*;
use pretty_assertions::assert_eq;

#[test]
fn apply_patch_manual_flow_snapshot() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    let mut proposed_changes = HashMap::new();
    proposed_changes.insert(
        PathBuf::from("foo.txt"),
        FileChange::Add {
            content: "hello\n".to_string(),
        },
    );
    chat.handle_codex_event(Event {
        id: "s1".into(),
        msg: EventMsg::ApplyPatchApprovalRequest(ApplyPatchApprovalRequestEvent {
            call_id: "c1".into(),
            turn_id: "turn-c1".into(),
            changes: proposed_changes,
            reason: Some("Manual review required".into()),
            grant_root: None,
        }),
    });
    let history_before_apply = drain_insert_history(&mut rx);
    assert!(
        history_before_apply.is_empty(),
        "expected approval modal to defer history emission"
    );

    let mut apply_changes = HashMap::new();
    apply_changes.insert(
        PathBuf::from("foo.txt"),
        FileChange::Add {
            content: "hello\n".to_string(),
        },
    );
    chat.handle_codex_event(Event {
        id: "s1".into(),
        msg: EventMsg::PatchApplyBegin(PatchApplyBeginEvent {
            call_id: "c1".into(),
            turn_id: "turn-c1".into(),
            auto_approved: false,
            changes: apply_changes,
        }),
    });
    let approved_lines = drain_insert_history(&mut rx)
        .pop()
        .expect("approved patch cell");

    assert_snapshot!(
        "apply_patch_manual_flow_history_approved",
        lines_to_single_string(&approved_lines)
    );
}

#[test]
fn apply_patch_approval_sends_op_with_submission_id() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    // Simulate receiving an approval request with a distinct submission id and call id
    let mut changes = HashMap::new();
    changes.insert(
        PathBuf::from("file.rs"),
        FileChange::Add {
            content: "fn main(){}\n".into(),
        },
    );
    let ev = ApplyPatchApprovalRequestEvent {
        call_id: "call-999".into(),
        turn_id: "turn-999".into(),
        changes,
        reason: None,
        grant_root: None,
    };
    chat.handle_codex_event(Event {
        id: "sub-123".into(),
        msg: EventMsg::ApplyPatchApprovalRequest(ev),
    });

    // Approve via key press 'y'
    chat.handle_key_event(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));

    // Expect a CodexOp with PatchApproval carrying the submission id, not call id
    let mut found = false;
    while let Ok(app_ev) = rx.try_recv() {
        if let AppEvent::CodexOp(Op::PatchApproval { id, decision }) = app_ev {
            assert_eq!(id, "sub-123");
            assert_matches!(decision, codex_core::protocol::ReviewDecision::Approved);
            found = true;
            break;
        }
    }
    assert!(found, "expected PatchApproval op to be sent");
}

#[test]
fn apply_patch_full_flow_integration_like() {
    let (mut chat, mut rx, mut op_rx) = make_chatwidget_manual();

    // 1) Backend requests approval
    let mut changes = HashMap::new();
    changes.insert(
        PathBuf::from("pkg.rs"),
        FileChange::Add { content: "".into() },
    );
    chat.handle_codex_event(Event {
        id: "sub-xyz".into(),
        msg: EventMsg::ApplyPatchApprovalRequest(ApplyPatchApprovalRequestEvent {
            call_id: "call-1".into(),
            turn_id: "turn-call-1".into(),
            changes,
            reason: None,
            grant_root: None,
        }),
    });

    // 2) User approves via 'y' and App receives a CodexOp
    chat.handle_key_event(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
    let mut maybe_op: Option<Op> = None;
    while let Ok(app_ev) = rx.try_recv() {
        if let AppEvent::CodexOp(op) = app_ev {
            maybe_op = Some(op);
            break;
        }
    }
    let op = maybe_op.expect("expected CodexOp after key press");

    // 3) App forwards to widget.submit_op, which pushes onto codex_op_tx
    chat.submit_op(op);
    let forwarded = op_rx
        .try_recv()
        .expect("expected op forwarded to codex channel");
    match forwarded {
        Op::PatchApproval { id, decision } => {
            assert_eq!(id, "sub-xyz");
            assert_matches!(decision, codex_core::protocol::ReviewDecision::Approved);
        }
        other => panic!("unexpected op forwarded: {other:?}"),
    }

    // 4) Simulate patch begin/end events from backend; ensure history cells are emitted
    let mut changes2 = HashMap::new();
    changes2.insert(
        PathBuf::from("pkg.rs"),
        FileChange::Add { content: "".into() },
    );
    chat.handle_codex_event(Event {
        id: "sub-xyz".into(),
        msg: EventMsg::PatchApplyBegin(PatchApplyBeginEvent {
            call_id: "call-1".into(),
            turn_id: "turn-call-1".into(),
            auto_approved: false,
            changes: changes2,
        }),
    });
    let mut end_changes = HashMap::new();
    end_changes.insert(
        PathBuf::from("pkg.rs"),
        FileChange::Add { content: "".into() },
    );
    chat.handle_codex_event(Event {
        id: "sub-xyz".into(),
        msg: EventMsg::PatchApplyEnd(PatchApplyEndEvent {
            call_id: "call-1".into(),
            turn_id: "turn-call-1".into(),
            stdout: String::from("ok"),
            stderr: String::new(),
            success: true,
            changes: end_changes,
        }),
    });
}

#[test]
fn apply_patch_untrusted_shows_approval_modal() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    // Ensure approval policy is untrusted (OnRequest)
    chat.config.approval_policy = AskForApproval::OnRequest;

    // Simulate a patch approval request from backend
    let mut changes = HashMap::new();
    changes.insert(
        PathBuf::from("a.rs"),
        FileChange::Add { content: "".into() },
    );
    chat.handle_codex_event(Event {
        id: "sub-1".into(),
        msg: EventMsg::ApplyPatchApprovalRequest(ApplyPatchApprovalRequestEvent {
            call_id: "call-1".into(),
            turn_id: "turn-call-1".into(),
            changes,
            reason: None,
            grant_root: None,
        }),
    });

    // Render and ensure the approval modal title is present
    let area = Rect::new(0, 0, 80, 12);
    let mut buf = Buffer::empty(area);
    chat.render(area, &mut buf);

    let mut contains_title = false;
    for y in 0..area.height {
        let mut row = String::new();
        for x in 0..area.width {
            row.push(buf[(x, y)].symbol().chars().next().unwrap_or(' '));
        }
        if row.contains("Would you like to make the following edits?") {
            contains_title = true;
            break;
        }
    }
    assert!(
        contains_title,
        "expected approval modal to be visible with title 'Would you like to make the following edits?'"
    );
}

#[test]
fn apply_patch_request_shows_diff_summary() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    // Ensure we are in OnRequest so an approval is surfaced
    chat.config.approval_policy = AskForApproval::OnRequest;

    // Simulate backend asking to apply a patch adding two lines to README.md
    let mut changes = HashMap::new();
    changes.insert(
        PathBuf::from("README.md"),
        FileChange::Add {
            // Two lines (no trailing empty line counted)
            content: "line one\nline two\n".into(),
        },
    );
    chat.handle_codex_event(Event {
        id: "sub-apply".into(),
        msg: EventMsg::ApplyPatchApprovalRequest(ApplyPatchApprovalRequestEvent {
            call_id: "call-apply".into(),
            turn_id: "turn-apply".into(),
            changes,
            reason: None,
            grant_root: None,
        }),
    });

    // No history entries yet; the modal should contain the diff summary
    let cells = drain_insert_history(&mut rx);
    assert!(
        cells.is_empty(),
        "expected approval request to render via modal instead of history"
    );

    let area = Rect::new(0, 0, 80, chat.desired_height(80));
    let mut buf = ratatui::buffer::Buffer::empty(area);
    chat.render(area, &mut buf);

    let mut saw_header = false;
    let mut saw_line1 = false;
    let mut saw_line2 = false;
    for y in 0..area.height {
        let mut row = String::new();
        for x in 0..area.width {
            row.push(buf[(x, y)].symbol().chars().next().unwrap_or(' '));
        }
        if row.contains("README.md (+2 -0)") {
            saw_header = true;
        }
        if row.contains("+line one") {
            saw_line1 = true;
        }
        if row.contains("+line two") {
            saw_line2 = true;
        }
        if saw_header && saw_line1 && saw_line2 {
            break;
        }
    }
    assert!(saw_header, "expected modal to show diff header with totals");
    assert!(
        saw_line1 && saw_line2,
        "expected modal to show per-line diff summary"
    );
}

#[test]
fn plan_update_renders_history_cell() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let update = UpdatePlanArgs {
        explanation: Some("Adapting plan".to_string()),
        plan: vec![
            PlanItemArg {
                step: "Explore codebase".into(),
                status: StepStatus::Completed,
            },
            PlanItemArg {
                step: "Implement feature".into(),
                status: StepStatus::InProgress,
            },
            PlanItemArg {
                step: "Write tests".into(),
                status: StepStatus::Pending,
            },
        ],
    };
    chat.handle_codex_event(Event {
        id: "sub-1".into(),
        msg: EventMsg::PlanUpdate(update),
    });
    let cells = drain_insert_history(&mut rx);
    assert!(!cells.is_empty(), "expected plan update cell to be sent");
    let blob = lines_to_single_string(cells.last().unwrap());
    assert!(
        blob.contains("Updated Plan"),
        "missing plan header: {blob:?}"
    );
    assert!(blob.contains("Explore codebase"));
    assert!(blob.contains("Implement feature"));
    assert!(blob.contains("Write tests"));
}

#[test]
fn plan_update_routes_to_pinned_drawer_when_enabled() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.set_plan_drawer_mode(PlanDrawerMode::Expanded);
    let update = UpdatePlanArgs {
        explanation: Some("Starting work".to_string()),
        plan: vec![
            PlanItemArg {
                step: "Research".into(),
                status: StepStatus::Completed,
            },
            PlanItemArg {
                step: "Implement".into(),
                status: StepStatus::InProgress,
            },
        ],
    };
    chat.handle_codex_event(Event {
        id: "sub-1".into(),
        msg: EventMsg::PlanUpdate(update),
    });
    // No history cell should be created when the pinned drawer is enabled.
    let cells = drain_insert_history(&mut rx);
    assert!(
        cells.is_empty(),
        "plan update should not create history cells when pinned drawer is enabled"
    );
    // The plan content should be visible in the rendered viewport.
    let rendered = render_bottom_popup(&chat, 60);
    assert!(
        rendered.contains("Research"),
        "rendered viewport should contain plan step text: {rendered:?}"
    );
    assert!(
        rendered.contains("Implement"),
        "rendered viewport should contain plan step text: {rendered:?}"
    );
}

#[test]
fn plan_update_routes_to_history_when_drawer_disabled() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    // plan_drawer_mode defaults to Off
    let update = UpdatePlanArgs {
        explanation: None,
        plan: vec![PlanItemArg {
            step: "Do something".into(),
            status: StepStatus::Pending,
        }],
    };
    chat.handle_codex_event(Event {
        id: "sub-1".into(),
        msg: EventMsg::PlanUpdate(update),
    });
    // History cell should be created when drawer is disabled.
    let cells = drain_insert_history(&mut rx);
    assert!(
        !cells.is_empty(),
        "plan update should create a history cell when pinned drawer is disabled"
    );
    // The plan content should NOT appear in the rendered viewport (it went to history/scrollback).
    let rendered = render_bottom_popup(&chat, 60);
    assert!(
        !rendered.contains("Do something"),
        "rendered viewport should not contain plan text when drawer is disabled: {rendered:?}"
    );
}

#[test]
fn normalized_plan_snapshot_routes_to_history_when_drawer_disabled() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.handle_client_event(nori_protocol::ClientEvent::PlanSnapshot(
        nori_protocol::PlanSnapshot {
            entries: vec![
                nori_protocol::PlanEntry {
                    step: "Research normalized plan handling".into(),
                    status: nori_protocol::PlanStatus::Completed,
                },
                nori_protocol::PlanEntry {
                    step: "Delete legacy plan translation".into(),
                    status: nori_protocol::PlanStatus::Pending,
                },
            ],
        },
    ));

    let cells = drain_insert_history(&mut rx);
    assert!(
        !cells.is_empty(),
        "normalized plan snapshots should create a history cell when the pinned drawer is disabled"
    );
    let blob = lines_to_single_string(cells.last().unwrap());
    assert!(
        blob.contains("Updated Plan"),
        "missing plan header: {blob:?}"
    );
    assert!(blob.contains("Research normalized plan handling"));
    assert!(blob.contains("Delete legacy plan translation"));
}

#[test]
fn toggling_pinned_drawer_off_routes_next_update_to_history() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.set_plan_drawer_mode(PlanDrawerMode::Expanded);
    // First update goes to the drawer.
    chat.handle_codex_event(Event {
        id: "sub-1".into(),
        msg: EventMsg::PlanUpdate(UpdatePlanArgs {
            explanation: None,
            plan: vec![PlanItemArg {
                step: "Task A".into(),
                status: StepStatus::InProgress,
            }],
        }),
    });
    let _ = drain_insert_history(&mut rx); // clear channel

    // Toggle off — drawer content should disappear from the viewport.
    chat.set_plan_drawer_mode(PlanDrawerMode::Off);
    let rendered = render_bottom_popup(&chat, 60);
    assert!(
        !rendered.contains("Task A"),
        "plan content should disappear from viewport after toggling drawer off: {rendered:?}"
    );

    // Next update should go to history, not the drawer.
    chat.handle_codex_event(Event {
        id: "sub-2".into(),
        msg: EventMsg::PlanUpdate(UpdatePlanArgs {
            explanation: None,
            plan: vec![PlanItemArg {
                step: "Task B".into(),
                status: StepStatus::Pending,
            }],
        }),
    });
    let cells = drain_insert_history(&mut rx);
    assert!(
        !cells.is_empty(),
        "plan update should create a history cell after drawer is toggled off"
    );
}

#[test]
fn toggling_pinned_drawer_on_shows_existing_plan() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    // Drawer is off by default. Send a plan update — it goes to history.
    chat.handle_codex_event(Event {
        id: "sub-1".into(),
        msg: EventMsg::PlanUpdate(UpdatePlanArgs {
            explanation: Some("Initial plan".to_string()),
            plan: vec![
                PlanItemArg {
                    step: "Step Alpha".into(),
                    status: StepStatus::Completed,
                },
                PlanItemArg {
                    step: "Step Beta".into(),
                    status: StepStatus::InProgress,
                },
            ],
        }),
    });
    let _ = drain_insert_history(&mut rx); // consume history cell

    // Now toggle the drawer on — the latest plan should appear in the viewport.
    chat.set_plan_drawer_mode(PlanDrawerMode::Expanded);
    let rendered = render_bottom_popup(&chat, 60);
    assert!(
        rendered.contains("Step Alpha"),
        "toggling drawer on should show the latest plan: {rendered:?}"
    );
    assert!(
        rendered.contains("Step Beta"),
        "toggling drawer on should show the latest plan: {rendered:?}"
    );
}

#[test]
fn toggling_pinned_drawer_off_then_on_preserves_plan() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.set_plan_drawer_mode(PlanDrawerMode::Expanded);
    // Send a plan while drawer is on.
    chat.handle_codex_event(Event {
        id: "sub-1".into(),
        msg: EventMsg::PlanUpdate(UpdatePlanArgs {
            explanation: None,
            plan: vec![PlanItemArg {
                step: "Persistent Task".into(),
                status: StepStatus::InProgress,
            }],
        }),
    });
    let _ = drain_insert_history(&mut rx);

    // Toggle off, then back on — plan should reappear.
    chat.set_plan_drawer_mode(PlanDrawerMode::Off);
    chat.set_plan_drawer_mode(PlanDrawerMode::Expanded);
    let rendered = render_bottom_popup(&chat, 60);
    assert!(
        rendered.contains("Persistent Task"),
        "plan should survive toggle off/on cycle: {rendered:?}"
    );
}

#[test]
fn collapsed_drawer_renders_one_line_summary() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    chat.set_plan_drawer_mode(PlanDrawerMode::Collapsed);
    chat.handle_codex_event(Event {
        id: "sub-1".into(),
        msg: EventMsg::PlanUpdate(UpdatePlanArgs {
            explanation: None,
            plan: vec![
                PlanItemArg {
                    step: "Explore codebase".into(),
                    status: StepStatus::Completed,
                },
                PlanItemArg {
                    step: "Implement feature".into(),
                    status: StepStatus::InProgress,
                },
                PlanItemArg {
                    step: "Write tests".into(),
                    status: StepStatus::Pending,
                },
            ],
        }),
    });
    let rendered = render_bottom_popup(&chat, 80);
    assert!(
        rendered.contains("1/3 completed"),
        "collapsed drawer should show progress count: {rendered:?}"
    );
    assert!(
        rendered.contains("Implement feature"),
        "collapsed drawer should show current step: {rendered:?}"
    );
}

#[test]
fn toggle_from_off_enters_collapsed() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    // Send a plan while drawer is off.
    chat.handle_codex_event(Event {
        id: "sub-1".into(),
        msg: EventMsg::PlanUpdate(UpdatePlanArgs {
            explanation: None,
            plan: vec![PlanItemArg {
                step: "Task One".into(),
                status: StepStatus::InProgress,
            }],
        }),
    });
    assert_eq!(chat.plan_drawer_mode(), PlanDrawerMode::Off);
    chat.toggle_plan_drawer();
    assert_eq!(chat.plan_drawer_mode(), PlanDrawerMode::Collapsed);

    let rendered = render_bottom_popup(&chat, 80);
    // Collapsed summary should be visible.
    assert!(
        rendered.contains("0/1 completed"),
        "toggling from Off should show collapsed summary: {rendered:?}"
    );
}

#[test]
fn toggle_from_collapsed_enters_expanded() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    chat.set_plan_drawer_mode(PlanDrawerMode::Collapsed);
    chat.handle_codex_event(Event {
        id: "sub-1".into(),
        msg: EventMsg::PlanUpdate(UpdatePlanArgs {
            explanation: Some("Starting".into()),
            plan: vec![
                PlanItemArg {
                    step: "Step A".into(),
                    status: StepStatus::Completed,
                },
                PlanItemArg {
                    step: "Step B".into(),
                    status: StepStatus::InProgress,
                },
            ],
        }),
    });
    chat.toggle_plan_drawer();
    assert_eq!(chat.plan_drawer_mode(), PlanDrawerMode::Expanded);

    let rendered = render_bottom_popup(&chat, 80);
    // Expanded view should show the full "Updated Plan" header and individual steps.
    assert!(
        rendered.contains("Updated Plan"),
        "expanded drawer should show full plan header: {rendered:?}"
    );
    assert!(
        rendered.contains("Step A"),
        "expanded drawer should show step details: {rendered:?}"
    );
    assert!(
        rendered.contains("Step B"),
        "expanded drawer should show step details: {rendered:?}"
    );
}

#[test]
fn toggle_from_expanded_enters_collapsed() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    chat.set_plan_drawer_mode(PlanDrawerMode::Expanded);
    chat.handle_codex_event(Event {
        id: "sub-1".into(),
        msg: EventMsg::PlanUpdate(UpdatePlanArgs {
            explanation: None,
            plan: vec![
                PlanItemArg {
                    step: "Alpha".into(),
                    status: StepStatus::Completed,
                },
                PlanItemArg {
                    step: "Beta".into(),
                    status: StepStatus::InProgress,
                },
                PlanItemArg {
                    step: "Gamma".into(),
                    status: StepStatus::Pending,
                },
            ],
        }),
    });
    chat.toggle_plan_drawer();
    assert_eq!(chat.plan_drawer_mode(), PlanDrawerMode::Collapsed);

    let rendered = render_bottom_popup(&chat, 80);
    // Collapsed should show summary, not "Updated Plan" header.
    assert!(
        !rendered.contains("Updated Plan"),
        "collapsed drawer should not show expanded header: {rendered:?}"
    );
    assert!(
        rendered.contains("1/3 completed"),
        "collapsed drawer should show progress: {rendered:?}"
    );
}

#[test]
fn collapsed_drawer_routes_updates_to_drawer_not_history() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.set_plan_drawer_mode(PlanDrawerMode::Collapsed);
    chat.handle_codex_event(Event {
        id: "sub-1".into(),
        msg: EventMsg::PlanUpdate(UpdatePlanArgs {
            explanation: None,
            plan: vec![PlanItemArg {
                step: "Some task".into(),
                status: StepStatus::Pending,
            }],
        }),
    });
    let cells = drain_insert_history(&mut rx);
    assert!(
        cells.is_empty(),
        "plan update should not create history cells when drawer is in Collapsed mode"
    );
}

#[test]
fn toggle_with_no_plan_is_safe() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    // No plan sent. Toggle should still work without panic.
    chat.toggle_plan_drawer();
    assert_eq!(chat.plan_drawer_mode(), PlanDrawerMode::Collapsed);

    let rendered = render_bottom_popup(&chat, 60);
    // No plan data, so nothing plan-related should be in the viewport.
    assert!(
        !rendered.contains("Plan:"),
        "collapsed drawer with no plan should not render: {rendered:?}"
    );
}

#[test]
fn collapsed_all_done_shows_completion_summary() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    chat.set_plan_drawer_mode(PlanDrawerMode::Collapsed);
    chat.handle_codex_event(Event {
        id: "sub-1".into(),
        msg: EventMsg::PlanUpdate(UpdatePlanArgs {
            explanation: None,
            plan: vec![
                PlanItemArg {
                    step: "Done one".into(),
                    status: StepStatus::Completed,
                },
                PlanItemArg {
                    step: "Done two".into(),
                    status: StepStatus::Completed,
                },
            ],
        }),
    });
    let rendered = render_bottom_popup(&chat, 80);
    assert!(
        rendered.contains("2/2 completed"),
        "all-completed should show full progress: {rendered:?}"
    );
    assert!(
        rendered.contains("All done"),
        "all-completed should show 'All done': {rendered:?}"
    );
}

#[test]
fn stream_error_updates_status_indicator() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.bottom_pane.set_task_running(true);
    let msg = "Reconnecting... 2/5";
    chat.handle_codex_event(Event {
        id: "sub-1".into(),
        msg: EventMsg::StreamError(StreamErrorEvent {
            message: msg.to_string(),
            codex_error_info: Some(CodexErrorInfo::Other),
        }),
    });

    let cells = drain_insert_history(&mut rx);
    assert!(
        cells.is_empty(),
        "expected no history cell for StreamError event"
    );
    let status = chat
        .bottom_pane
        .status_widget()
        .expect("status indicator should be visible");
    assert_eq!(status.header(), msg);
}

#[test]
fn warning_event_adds_warning_history_cell() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.handle_codex_event(Event {
        id: "sub-1".into(),
        msg: EventMsg::Warning(WarningEvent {
            message: "test warning message".to_string(),
        }),
    });

    let cells = drain_insert_history(&mut rx);
    assert_eq!(cells.len(), 1, "expected one warning history cell");
    let rendered = lines_to_single_string(&cells[0]);
    assert!(
        rendered.contains("test warning message"),
        "warning cell missing content: {rendered}"
    );
}

#[test]
fn multiple_agent_messages_in_single_turn_emit_multiple_headers() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    // Begin turn
    chat.handle_codex_event(Event {
        id: "s1".into(),
        msg: EventMsg::TaskStarted(TaskStartedEvent {
            model_context_window: None,
        }),
    });

    // First finalized assistant message
    chat.handle_codex_event(Event {
        id: "s1".into(),
        msg: EventMsg::AgentMessage(AgentMessageEvent {
            message: "First message".into(),
        }),
    });

    // Second finalized assistant message in the same turn
    chat.handle_codex_event(Event {
        id: "s1".into(),
        msg: EventMsg::AgentMessage(AgentMessageEvent {
            message: "Second message".into(),
        }),
    });

    // End turn
    chat.handle_codex_event(Event {
        id: "s1".into(),
        msg: EventMsg::TaskComplete(TaskCompleteEvent {
            last_agent_message: None,
        }),
    });

    let cells = drain_insert_history(&mut rx);
    let combined: String = cells
        .iter()
        .map(|lines| lines_to_single_string(lines))
        .collect();
    assert!(
        combined.contains("First message"),
        "missing first message: {combined}"
    );
    assert!(
        combined.contains("Second message"),
        "missing second message: {combined}"
    );
    let first_idx = combined.find("First message").unwrap();
    let second_idx = combined.find("Second message").unwrap();
    assert!(first_idx < second_idx, "messages out of order: {combined}");
}

#[test]
fn final_reasoning_then_message_without_deltas_are_rendered() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    // No deltas; only final reasoning followed by final message.
    chat.handle_codex_event(Event {
        id: "s1".into(),
        msg: EventMsg::AgentReasoning(AgentReasoningEvent {
            text: "I will first analyze the request.".into(),
        }),
    });
    chat.handle_codex_event(Event {
        id: "s1".into(),
        msg: EventMsg::AgentMessage(AgentMessageEvent {
            message: "Here is the result.".into(),
        }),
    });

    // Drain history and snapshot the combined visible content.
    let cells = drain_insert_history(&mut rx);
    let combined = cells
        .iter()
        .map(|lines| lines_to_single_string(lines))
        .collect::<String>();
    assert_snapshot!(combined);
}

#[test]
fn deltas_then_same_final_message_are_rendered_snapshot() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    // Stream some reasoning deltas first.
    chat.handle_codex_event(Event {
        id: "s1".into(),
        msg: EventMsg::AgentReasoningDelta(AgentReasoningDeltaEvent {
            delta: "I will ".into(),
        }),
    });
    chat.handle_codex_event(Event {
        id: "s1".into(),
        msg: EventMsg::AgentReasoningDelta(AgentReasoningDeltaEvent {
            delta: "first analyze the ".into(),
        }),
    });
    chat.handle_codex_event(Event {
        id: "s1".into(),
        msg: EventMsg::AgentReasoningDelta(AgentReasoningDeltaEvent {
            delta: "request.".into(),
        }),
    });
    chat.handle_codex_event(Event {
        id: "s1".into(),
        msg: EventMsg::AgentReasoning(AgentReasoningEvent {
            text: "request.".into(),
        }),
    });

    // Then stream answer deltas, followed by the exact same final message.
    chat.handle_codex_event(Event {
        id: "s1".into(),
        msg: EventMsg::AgentMessageDelta(AgentMessageDeltaEvent {
            delta: "Here is the ".into(),
        }),
    });
    chat.handle_codex_event(Event {
        id: "s1".into(),
        msg: EventMsg::AgentMessageDelta(AgentMessageDeltaEvent {
            delta: "result.".into(),
        }),
    });

    chat.handle_codex_event(Event {
        id: "s1".into(),
        msg: EventMsg::AgentMessage(AgentMessageEvent {
            message: "Here is the result.".into(),
        }),
    });

    // Snapshot the combined visible content to ensure we render as expected
    // when deltas are followed by the identical final message.
    let cells = drain_insert_history(&mut rx);
    let combined = cells
        .iter()
        .map(|lines| lines_to_single_string(lines))
        .collect::<String>();
    assert_snapshot!(combined);
}

#[test]
fn chatwidget_exec_and_status_layout_vt100_snapshot() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.handle_codex_event(Event {
        id: "t1".into(),
        msg: EventMsg::AgentMessage(AgentMessageEvent { message: "I’m going to search the repo for where “Change Approved” is rendered to update that view.".into() }),
    });

    let command = vec!["bash".into(), "-lc".into(), "rg \"Change Approved\"".into()];
    let parsed_cmd = vec![
        ParsedCommand::Search {
            query: Some("Change Approved".into()),
            path: None,
            cmd: "rg \"Change Approved\"".into(),
        },
        ParsedCommand::Read {
            name: "diff_render.rs".into(),
            cmd: "cat diff_render.rs".into(),
            path: "diff_render.rs".into(),
        },
    ];
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    chat.handle_codex_event(Event {
        id: "c1".into(),
        msg: EventMsg::ExecCommandBegin(ExecCommandBeginEvent {
            call_id: "c1".into(),
            process_id: None,
            turn_id: "turn-1".into(),
            command: command.clone(),
            cwd: cwd.clone(),
            parsed_cmd: parsed_cmd.clone(),
            source: ExecCommandSource::Agent,
            interaction_input: None,
        }),
    });
    chat.handle_codex_event(Event {
        id: "c1".into(),
        msg: EventMsg::ExecCommandEnd(ExecCommandEndEvent {
            call_id: "c1".into(),
            process_id: None,
            turn_id: "turn-1".into(),
            command,
            cwd,
            parsed_cmd,
            source: ExecCommandSource::Agent,
            interaction_input: None,
            stdout: String::new(),
            stderr: String::new(),
            aggregated_output: String::new(),
            exit_code: 0,
            duration: std::time::Duration::from_millis(16000),
            formatted_output: String::new(),
        }),
    });
    chat.handle_codex_event(Event {
        id: "t1".into(),
        msg: EventMsg::TaskStarted(TaskStartedEvent {
            model_context_window: None,
        }),
    });
    chat.handle_codex_event(Event {
        id: "t1".into(),
        msg: EventMsg::AgentReasoningDelta(AgentReasoningDeltaEvent {
            delta: "**Investigating rendering code**".into(),
        }),
    });
    chat.bottom_pane
        .set_composer_text("Summarize recent commits".to_string());

    let width: u16 = 80;
    let ui_height: u16 = chat.desired_height(width);
    let vt_height: u16 = 40;
    let viewport = Rect::new(0, vt_height - ui_height - 1, width, ui_height);

    let backend = VT100Backend::new(width, vt_height);
    let mut term = crate::custom_terminal::Terminal::with_options(backend).expect("terminal");
    term.set_viewport_area(viewport);

    for lines in drain_insert_history(&mut rx) {
        crate::insert_history::insert_history_lines(&mut term, lines)
            .expect("Failed to insert history lines in test");
    }

    term.draw(|f| {
        chat.render(f.area(), f.buffer_mut());
    })
    .unwrap();

    assert_snapshot!(term.backend().vt100().screen().contents());
}

#[test]
fn chatwidget_markdown_code_blocks_vt100_snapshot() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    // Simulate a final agent message via streaming deltas instead of a single message

    chat.handle_codex_event(Event {
        id: "t1".into(),
        msg: EventMsg::TaskStarted(TaskStartedEvent {
            model_context_window: None,
        }),
    });
    // Build a vt100 visual from the history insertions only (no UI overlay)
    let width: u16 = 80;
    let height: u16 = 50;
    let backend = VT100Backend::new(width, height);
    let mut term = crate::custom_terminal::Terminal::with_options(backend).expect("terminal");
    // Place viewport at the last line so that history lines insert above it
    term.set_viewport_area(Rect::new(0, height - 1, width, 1));

    // Simulate streaming via AgentMessageDelta in 2-character chunks (no final AgentMessage).
    let source: &str = r#"

    -- Indented code block (4 spaces)
    SELECT *
    FROM "users"
    WHERE "email" LIKE '%@example.com';

````markdown
```sh
printf 'fenced within fenced\n'
```
````

```jsonc
{
  // comment allowed in jsonc
  "path": "C:\\Program Files\\App",
  "regex": "^foo.*(bar)?$"
}
```
"#;

    let mut it = source.chars();
    loop {
        let mut delta = String::new();
        match it.next() {
            Some(c) => delta.push(c),
            None => break,
        }
        if let Some(c2) = it.next() {
            delta.push(c2);
        }

        chat.handle_codex_event(Event {
            id: "t1".into(),
            msg: EventMsg::AgentMessageDelta(AgentMessageDeltaEvent { delta }),
        });
        // Drive commit ticks and drain emitted history lines into the vt100 buffer.
        loop {
            chat.on_commit_tick();
            let mut inserted_any = false;
            while let Ok(app_ev) = rx.try_recv() {
                if let AppEvent::InsertHistoryCell(cell) = app_ev {
                    let lines = cell.display_lines(width);
                    crate::insert_history::insert_history_lines(&mut term, lines)
                        .expect("Failed to insert history lines in test");
                    inserted_any = true;
                }
            }
            if !inserted_any {
                break;
            }
        }
    }

    // Finalize the stream without sending a final AgentMessage, to flush any tail.
    chat.handle_codex_event(Event {
        id: "t1".into(),
        msg: EventMsg::TaskComplete(TaskCompleteEvent {
            last_agent_message: None,
        }),
    });
    for lines in drain_insert_history(&mut rx) {
        crate::insert_history::insert_history_lines(&mut term, lines)
            .expect("Failed to insert history lines in test");
    }

    assert_snapshot!(term.backend().vt100().screen().contents());
}

#[test]
fn chatwidget_tall() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    chat.handle_codex_event(Event {
        id: "t1".into(),
        msg: EventMsg::TaskStarted(TaskStartedEvent {
            model_context_window: None,
        }),
    });
    chat.bottom_pane
        .update_status_header("Thinking really hard".to_string());
    for i in 0..30 {
        chat.queue_user_message(format!("Hello, world! {i}").into());
    }
    let width: u16 = 80;
    let height: u16 = 24;
    let backend = VT100Backend::new(width, height);
    let mut term = crate::custom_terminal::Terminal::with_options(backend).expect("terminal");
    let desired_height = chat.desired_height(width).min(height);
    term.set_viewport_area(Rect::new(0, height - desired_height, width, desired_height));
    term.draw(|f| {
        chat.render(f.area(), f.buffer_mut());
    })
    .unwrap();
    assert_snapshot!(term.backend().vt100().screen().contents());
}

/// Blackbox test: type "hello" into the composer and snapshot the result.
#[test]
fn blackbox_typing_snapshot() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    // Set text in the composer (simulating typing)
    chat.bottom_pane.set_composer_text("hello".to_string());

    // Render to a test terminal
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("create terminal");
    terminal
        .draw(|f| chat.render(f.area(), f.buffer_mut()))
        .expect("draw chat with typed text");

    assert_snapshot!("blackbox_typing_hello", terminal.backend());
}

/// Blackbox test: open the /model picker and snapshot the result.
#[test]
fn blackbox_model_picker_snapshot() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    // Open the model picker popup
    chat.open_model_popup();

    // Render to a test terminal
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("create terminal");
    terminal
        .draw(|f| chat.render(f.area(), f.buffer_mut()))
        .expect("draw chat with model picker");

    assert_snapshot!("blackbox_model_picker_open", terminal.backend());
}

/// ACP agents stream text responses character-by-character. This test verifies
/// that streamed text from an ACP agent renders correctly in the chat history.
#[test]
fn acp_streaming_text_response_vt100_snapshot() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    // Start task (simulates ACP session initialization)
    chat.handle_codex_event(Event {
        id: "acp-1".into(),
        msg: EventMsg::TaskStarted(TaskStartedEvent {
            model_context_window: None,
        }),
    });

    // Build vt100 terminal for visual snapshot
    let width: u16 = 80;
    let height: u16 = 30;
    let backend = VT100Backend::new(width, height);
    let mut term = crate::custom_terminal::Terminal::with_options(backend).expect("terminal");
    term.set_viewport_area(Rect::new(0, height - 1, width, 1));

    // Simulate ACP streaming response - typical agent response with code
    let acp_response = r#"I'll help you implement that feature.

Here's the code:

```rust
fn calculate_sum(numbers: &[i32]) -> i32 {
    numbers.iter().sum()
}
```

This function takes a slice of integers and returns their sum using iterator methods."#;

    // Stream in character pairs (simulating ACP notification delivery)
    let mut chars = acp_response.chars();
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
            id: "acp-1".into(),
            msg: EventMsg::AgentMessageDelta(AgentMessageDeltaEvent { delta }),
        });

        // Process commit ticks and drain history
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

    // Finalize stream (simulates ACP session completion)
    chat.handle_codex_event(Event {
        id: "acp-1".into(),
        msg: EventMsg::TaskComplete(TaskCompleteEvent {
            last_agent_message: None,
        }),
    });
    for lines in drain_insert_history(&mut rx) {
        crate::insert_history::insert_history_lines(&mut term, lines)
            .expect("Failed to insert final history lines");
    }

    assert_snapshot!(
        "acp_streaming_text_response",
        term.backend().vt100().screen().contents()
    );
}

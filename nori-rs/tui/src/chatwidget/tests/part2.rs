use super::*;
use pretty_assertions::assert_eq;

use codex_core::protocol::ThreadGoalStatus;

#[test]
fn slash_quit_sends_shutdown() {
    let (mut chat, _rx, mut op_rx) = make_chatwidget_manual();

    chat.dispatch_command(SlashCommand::Quit);

    assert_matches!(op_rx.try_recv(), Ok(Op::Shutdown));
}

#[test]
fn slash_exit_sends_shutdown() {
    let (mut chat, _rx, mut op_rx) = make_chatwidget_manual();

    chat.dispatch_command(SlashCommand::Exit);

    assert_matches!(op_rx.try_recv(), Ok(Op::Shutdown));
}

#[test]
fn slash_undo_sends_op() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.dispatch_command(SlashCommand::Undo);

    match rx.try_recv() {
        Ok(AppEvent::CodexOp(Op::UndoList)) => {}
        other => panic!("expected AppEvent::CodexOp(Op::UndoList), got {other:?}"),
    }
}

#[test]
fn slash_goal_requests_current_goal() {
    let (mut chat, _rx, mut op_rx) = make_chatwidget_manual();

    chat.dispatch_command(SlashCommand::Goal);

    assert_matches!(op_rx.try_recv(), Ok(Op::ThreadGoalGet));
}

#[test]
fn slash_goal_is_disabled_when_goal_tools_are_unsupported() {
    let (mut chat, mut rx, mut op_rx) = make_chatwidget_manual();
    chat.handle_client_event(nori_protocol::ClientEvent::SessionCapabilitiesChanged(
        goal_capabilities(false),
    ));

    chat.dispatch_command(SlashCommand::Goal);

    assert_matches!(op_rx.try_recv(), Err(TryRecvError::Empty));
    let cells = drain_insert_history(&mut rx);
    assert!(
        !cells.is_empty(),
        "expected disabled goal command to emit a user-visible message"
    );
    let rendered = lines_to_single_string(cells.last().unwrap());
    assert!(
        rendered.contains("/goal is unavailable"),
        "expected disabled goal explanation, got: {rendered}"
    );
}

#[test]
fn typed_goal_command_is_disabled_when_goal_tools_are_unsupported() {
    let (mut chat, mut rx, mut op_rx) = make_chatwidget_manual();
    chat.handle_client_event(nori_protocol::ClientEvent::SessionCapabilitiesChanged(
        goal_capabilities(false),
    ));

    chat.submit_user_message("/goal Ship this".to_string().into());

    assert_prompt_history_entry(&mut op_rx, "/goal Ship this");
    assert_matches!(op_rx.try_recv(), Err(TryRecvError::Empty));
    let cells = drain_insert_history(&mut rx);
    assert!(
        !cells.is_empty(),
        "expected typed disabled goal command to emit a user-visible message"
    );
    let rendered = lines_to_single_string(cells.last().unwrap());
    assert!(
        rendered.contains("/goal is unavailable"),
        "expected disabled goal explanation, got: {rendered}"
    );
}

fn goal_capabilities(goal_enabled: bool) -> nori_protocol::SessionCapabilitiesView {
    nori_protocol::SessionCapabilitiesView {
        agent: nori_protocol::AgentCapabilitiesView {
            http_mcp: goal_enabled,
            load_session: true,
            session_list: false,
        },
        nori_client: nori_protocol::NoriClientCapabilitiesView {
            advertised: goal_enabled,
            initialized: false,
        },
        builtin_commands: std::collections::HashMap::from([(
            "goal".to_string(),
            nori_protocol::CommandAvailability {
                enabled: goal_enabled,
                reason: (!goal_enabled)
                    .then(|| "The active agent does not support HTTP MCP.".to_string()),
            },
        )]),
    }
}

#[test]
fn slash_picker_goal_renders_current_goal_summary() {
    let (mut chat, mut rx, mut op_rx) = make_chatwidget_manual();
    let goal = test_thread_goal("Keep going", nori_protocol::ThreadGoalStatus::Active);
    chat.handle_client_event(nori_protocol::ClientEvent::ThreadGoalUpdated(
        nori_protocol::ThreadGoalUpdated { goal: goal.clone() },
    ));
    let _ = drain_insert_history(&mut rx);

    chat.dispatch_command(SlashCommand::Goal);
    assert_eq!(op_rx.try_recv(), Ok(Op::ThreadGoalGet));
    chat.handle_client_event(nori_protocol::ClientEvent::ThreadGoalUpdated(
        nori_protocol::ThreadGoalUpdated { goal },
    ));

    let cells = drain_insert_history(&mut rx);
    assert_eq!(cells.len(), 1);
    let rendered = lines_to_single_string(&cells[0]);
    assert!(
        rendered.contains("Objective: Keep going"),
        "expected slash picker goal summary, got: {rendered}"
    );
}

#[test]
fn goal_objective_submits_thread_goal_set() {
    let (mut chat, _rx, mut op_rx) = make_chatwidget_manual();

    chat.submit_user_message("/goal Ship the ACP goal command".to_string().into());

    assert_eq!(
        op_rx.try_recv(),
        Ok(Op::ThreadGoalSet {
            objective: Some("Ship the ACP goal command".to_string()),
            status: Some(ThreadGoalStatus::Active),
        })
    );
}

#[test]
fn goal_objective_is_added_to_prompt_history() {
    let (mut chat, _rx, mut op_rx) = make_chatwidget_manual();

    chat.submit_user_message("/goal Ship the ACP goal command".to_string().into());

    assert_eq!(
        op_rx.try_recv(),
        Ok(Op::ThreadGoalSet {
            objective: Some("Ship the ACP goal command".to_string()),
            status: Some(ThreadGoalStatus::Active),
        })
    );
    assert_prompt_history_entry(&mut op_rx, "/goal Ship the ACP goal command");
}

#[test]
fn goal_objective_confirms_before_replacing_unfinished_goal() {
    let (mut chat, _rx, mut op_rx) = make_chatwidget_manual();
    chat.handle_client_event(nori_protocol::ClientEvent::ThreadGoalUpdated(
        nori_protocol::ThreadGoalUpdated {
            goal: test_thread_goal("Existing goal", nori_protocol::ThreadGoalStatus::Active),
        },
    ));

    chat.submit_user_message("/goal Replacement goal".to_string().into());

    assert_prompt_history_entry(&mut op_rx, "/goal Replacement goal");
    assert_matches!(op_rx.try_recv(), Err(TryRecvError::Empty));
    let popup = render_bottom_popup(&chat, 80);
    assert_snapshot!("goal_replace_confirmation_popup", popup);
}

#[test]
fn goal_replace_confirmation_submits_new_objective() {
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;

    let (mut chat, mut rx, mut op_rx) = make_chatwidget_manual();
    chat.handle_client_event(nori_protocol::ClientEvent::ThreadGoalUpdated(
        nori_protocol::ThreadGoalUpdated {
            goal: test_thread_goal("Existing goal", nori_protocol::ThreadGoalStatus::Paused),
        },
    ));
    let _ = drain_insert_history(&mut rx);

    chat.submit_user_message("/goal Replacement goal".to_string().into());
    chat.handle_key_event(KeyEvent::from(KeyCode::Enter));

    loop {
        match rx.try_recv() {
            Ok(AppEvent::CodexOp(Op::ThreadGoalSet {
                objective: Some(objective),
                status: Some(ThreadGoalStatus::Active),
            })) => {
                assert_eq!(objective, "Replacement goal");
                break;
            }
            Ok(_) => {}
            other => panic!("expected replacement ThreadGoalSet event, got {other:?}"),
        }
    }
    assert_prompt_history_entry(&mut op_rx, "/goal Replacement goal");
    assert_matches!(op_rx.try_recv(), Err(TryRecvError::Empty));
}

#[test]
fn goal_objective_replaces_completed_goal_without_confirmation() {
    let (mut chat, _rx, mut op_rx) = make_chatwidget_manual();
    chat.handle_client_event(nori_protocol::ClientEvent::ThreadGoalUpdated(
        nori_protocol::ThreadGoalUpdated {
            goal: test_thread_goal("Finished goal", nori_protocol::ThreadGoalStatus::Complete),
        },
    ));

    chat.submit_user_message("/goal Next goal".to_string().into());

    assert_eq!(
        op_rx.try_recv(),
        Ok(Op::ThreadGoalSet {
            objective: Some("Next goal".to_string()),
            status: Some(ThreadGoalStatus::Active),
        })
    );
}

#[test]
fn goal_status_commands_submit_goal_mutations() {
    let cases = [
        (
            "/goal pause",
            Op::ThreadGoalSet {
                objective: None,
                status: Some(ThreadGoalStatus::Paused),
            },
        ),
        (
            "/goal resume",
            Op::ThreadGoalSet {
                objective: None,
                status: Some(ThreadGoalStatus::Active),
            },
        ),
        ("/goal clear", Op::ThreadGoalClear),
    ];

    for (input, expected) in cases {
        let (mut chat, _rx, mut op_rx) = make_chatwidget_manual();

        chat.submit_user_message(input.to_string().into());

        assert_eq!(op_rx.try_recv(), Ok(expected));
        assert_prompt_history_entry(&mut op_rx, input);
    }
}

#[test]
fn goal_update_event_renders_summary() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.handle_client_event(nori_protocol::ClientEvent::ThreadGoalUpdated(
        nori_protocol::ThreadGoalUpdated {
            goal: nori_protocol::ThreadGoal {
                time_used_seconds: 63,
                ..test_thread_goal_with_tokens(
                    "Keep going",
                    nori_protocol::ThreadGoalStatus::Active,
                    1_060,
                )
            },
        },
    ));

    let cells = drain_insert_history(&mut rx);
    assert_eq!(cells.len(), 1);
    let rendered = lines_to_single_string(&cells[0]);
    assert_snapshot!("goal_update_event_summary", rendered);
}

#[test]
fn accounting_only_goal_update_does_not_render_history_cell() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.handle_client_event(nori_protocol::ClientEvent::ThreadGoalUpdated(
        nori_protocol::ThreadGoalUpdated {
            goal: test_thread_goal("Keep going", nori_protocol::ThreadGoalStatus::Active),
        },
    ));
    let _ = drain_insert_history(&mut rx);

    chat.handle_client_event(nori_protocol::ClientEvent::ThreadGoalUpdated(
        nori_protocol::ThreadGoalUpdated {
            goal: nori_protocol::ThreadGoal {
                tokens_used: 195_043,
                time_used_seconds: 15,
                updated_at: 25,
                ..test_thread_goal("Keep going", nori_protocol::ThreadGoalStatus::Active)
            },
        },
    ));

    assert_eq!(drain_insert_history(&mut rx).len(), 0);
}

#[test]
fn explicit_goal_status_request_renders_current_goal_summary() {
    let (mut chat, mut rx, mut op_rx) = make_chatwidget_manual();
    let goal = test_thread_goal("Keep going", nori_protocol::ThreadGoalStatus::Active);
    chat.handle_client_event(nori_protocol::ClientEvent::ThreadGoalUpdated(
        nori_protocol::ThreadGoalUpdated { goal: goal.clone() },
    ));
    let _ = drain_insert_history(&mut rx);

    chat.submit_user_message("/goal".to_string().into());
    assert_eq!(op_rx.try_recv(), Ok(Op::ThreadGoalGet));
    assert_prompt_history_entry(&mut op_rx, "/goal");
    chat.handle_client_event(nori_protocol::ClientEvent::ThreadGoalUpdated(
        nori_protocol::ThreadGoalUpdated { goal },
    ));

    let cells = drain_insert_history(&mut rx);
    assert_eq!(cells.len(), 1);
    let rendered = lines_to_single_string(&cells[0]);
    assert!(
        rendered.contains("Objective: Keep going"),
        "expected explicit goal status summary, got: {rendered}"
    );
}

#[test]
fn status_goal_update_still_renders_history_cell() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.handle_client_event(nori_protocol::ClientEvent::ThreadGoalUpdated(
        nori_protocol::ThreadGoalUpdated {
            goal: test_thread_goal("Keep going", nori_protocol::ThreadGoalStatus::Active),
        },
    ));
    let _ = drain_insert_history(&mut rx);

    chat.handle_client_event(nori_protocol::ClientEvent::ThreadGoalUpdated(
        nori_protocol::ThreadGoalUpdated {
            goal: test_thread_goal("Keep going", nori_protocol::ThreadGoalStatus::Paused),
        },
    ));

    let cells = drain_insert_history(&mut rx);
    assert_eq!(cells.len(), 1);
    let rendered = lines_to_single_string(&cells[0]);
    assert!(
        rendered.contains("Status: paused"),
        "expected paused status summary, got: {rendered}"
    );
}

#[test]
fn goal_edit_prefills_current_goal_objective() {
    let (mut chat, _rx, mut op_rx) = make_chatwidget_manual();
    chat.handle_client_event(nori_protocol::ClientEvent::ThreadGoalUpdated(
        nori_protocol::ThreadGoalUpdated {
            goal: test_thread_goal(
                "Keep improving the ACP goal command",
                nori_protocol::ThreadGoalStatus::Paused,
            ),
        },
    ));

    chat.submit_user_message("/goal edit".to_string().into());

    assert_eq!(
        chat.bottom_pane.composer_text(),
        "/goal Keep improving the ACP goal command"
    );
    assert_prompt_history_entry(&mut op_rx, "/goal edit");
    assert_matches!(op_rx.try_recv(), Err(TryRecvError::Empty));
}

#[test]
fn goal_edit_without_goal_does_not_open_editor_on_later_goal_update() {
    let (mut chat, mut rx, mut op_rx) = make_chatwidget_manual();

    chat.submit_user_message("/goal edit".to_string().into());
    assert_matches!(op_rx.try_recv(), Ok(Op::ThreadGoalGet));
    assert_prompt_history_entry(&mut op_rx, "/goal edit");

    chat.handle_client_event(nori_protocol::ClientEvent::SessionUpdateInfo(
        nori_protocol::SessionUpdateInfo {
            kind: nori_protocol::SessionUpdateKind::SessionInfo,
            message: "Usage: /goal <objective>".to_string(),
            hint: Some("No goal is currently set.".to_string()),
            usage: None,
        },
    ));

    let cells = drain_insert_history(&mut rx);
    let rendered = cells
        .iter()
        .map(|cell| lines_to_single_string(cell))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        rendered.contains("No goal is currently set."),
        "expected no-goal hint, got: {rendered}"
    );

    chat.handle_client_event(nori_protocol::ClientEvent::ThreadGoalUpdated(
        nori_protocol::ThreadGoalUpdated {
            goal: test_thread_goal("Later goal", nori_protocol::ThreadGoalStatus::Active),
        },
    ));

    assert_ne!(chat.bottom_pane.composer_text(), "/goal Later goal");
}

fn test_thread_goal(
    objective: &str,
    status: nori_protocol::ThreadGoalStatus,
) -> nori_protocol::ThreadGoal {
    test_thread_goal_with_tokens(objective, status, 0)
}

fn test_thread_goal_with_tokens(
    objective: &str,
    status: nori_protocol::ThreadGoalStatus,
    tokens_used: i64,
) -> nori_protocol::ThreadGoal {
    nori_protocol::ThreadGoal {
        objective: objective.to_string(),
        status,
        tokens_used,
        time_used_seconds: 0,
        created_at: 10,
        updated_at: 10,
    }
}

fn assert_prompt_history_entry(
    op_rx: &mut tokio::sync::mpsc::UnboundedReceiver<Op>,
    expected_text: &str,
) {
    assert_eq!(
        op_rx.try_recv(),
        Ok(Op::AddToHistory {
            text: expected_text.to_string(),
        })
    );
}

#[test]
fn slash_first_prompt_shows_initial_prompt() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.first_prompt_text = Some("build me a website".to_string());

    chat.dispatch_command(SlashCommand::FirstPrompt);

    let cells = drain_insert_history(&mut rx);
    assert_eq!(cells.len(), 1);
    let rendered = lines_to_single_string(&cells[0]);
    assert!(
        rendered.contains("build me a website"),
        "expected first prompt text in output, got: {rendered}"
    );
}

#[test]
fn slash_first_prompt_shows_fallback_when_none() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.dispatch_command(SlashCommand::FirstPrompt);

    let cells = drain_insert_history(&mut rx);
    assert_eq!(cells.len(), 1);
    let rendered = lines_to_single_string(&cells[0]);
    assert!(
        rendered.contains("No prompt has been submitted yet"),
        "expected fallback message, got: {rendered}"
    );
}

#[test]
fn undo_success_events_render_info_messages() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.handle_codex_event(Event {
        id: "turn-1".to_string(),
        msg: EventMsg::UndoStarted(UndoStartedEvent {
            message: Some("Undo requested for the last turn...".to_string()),
        }),
    });
    assert!(
        chat.bottom_pane.status_indicator_visible(),
        "status indicator should be visible during undo"
    );

    chat.handle_codex_event(Event {
        id: "turn-1".to_string(),
        msg: EventMsg::UndoCompleted(UndoCompletedEvent {
            success: true,
            message: None,
        }),
    });

    let cells = drain_insert_history(&mut rx);
    assert_eq!(cells.len(), 1, "expected final status only");
    assert!(
        !chat.bottom_pane.status_indicator_visible(),
        "status indicator should be hidden after successful undo"
    );

    let completed = lines_to_single_string(&cells[0]);
    assert!(
        completed.contains("Undo completed successfully."),
        "expected default success message, got {completed:?}"
    );
}

#[test]
fn undo_failure_events_render_error_message() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.handle_codex_event(Event {
        id: "turn-2".to_string(),
        msg: EventMsg::UndoStarted(UndoStartedEvent { message: None }),
    });
    assert!(
        chat.bottom_pane.status_indicator_visible(),
        "status indicator should be visible during undo"
    );

    chat.handle_codex_event(Event {
        id: "turn-2".to_string(),
        msg: EventMsg::UndoCompleted(UndoCompletedEvent {
            success: false,
            message: Some("Failed to restore workspace state.".to_string()),
        }),
    });

    let cells = drain_insert_history(&mut rx);
    assert_eq!(cells.len(), 1, "expected final status only");
    assert!(
        !chat.bottom_pane.status_indicator_visible(),
        "status indicator should be hidden after failed undo"
    );

    let completed = lines_to_single_string(&cells[0]);
    assert!(
        completed.contains("Failed to restore workspace state."),
        "expected failure message, got {completed:?}"
    );
}

#[test]
fn undo_started_hides_interrupt_hint() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    chat.handle_codex_event(Event {
        id: "turn-hint".to_string(),
        msg: EventMsg::UndoStarted(UndoStartedEvent { message: None }),
    });

    let status = chat
        .bottom_pane
        .status_widget()
        .expect("status indicator should be active");
    assert!(
        !status.interrupt_hint_visible(),
        "undo should hide the interrupt hint because the operation cannot be cancelled"
    );
}

#[test]
fn view_image_tool_call_adds_history_cell() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let image_path = chat.config.cwd.join("example.png");

    chat.handle_codex_event(Event {
        id: "sub-image".into(),
        msg: EventMsg::ViewImageToolCall(ViewImageToolCallEvent {
            call_id: "call-image".into(),
            path: image_path,
        }),
    });

    let cells = drain_insert_history(&mut rx);
    assert_eq!(cells.len(), 1, "expected a single history cell");
    let combined = lines_to_single_string(&cells[0]);
    assert_snapshot!("local_image_attachment_history_snapshot", combined);
}

#[test]
fn interrupt_exec_marks_failed_snapshot() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    // Begin a long-running command so we have an active exec cell with a spinner.
    begin_exec(&mut chat, "call-int", "sleep 1");

    // Simulate the task being aborted (as if ESC was pressed), which should
    // cause the active exec cell to be finalized as failed and flushed.
    chat.handle_codex_event(Event {
        id: "call-int".into(),
        msg: EventMsg::TurnAborted(codex_core::protocol::TurnAbortedEvent {
            reason: TurnAbortReason::Interrupted,
        }),
    });

    let cells = drain_insert_history(&mut rx);
    assert!(
        !cells.is_empty(),
        "expected finalized exec cell to be inserted into history"
    );

    // The first inserted cell should be the finalized exec; snapshot its text.
    let exec_blob = lines_to_single_string(&cells[0]);
    assert_snapshot!("interrupt_exec_marks_failed", exec_blob);
}

#[test]
fn model_selection_popup_snapshot() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    chat.config.model = "gpt-5-codex".to_string();
    chat.open_model_popup();

    let popup = render_bottom_popup(&chat, 80);
    assert_snapshot!("model_selection_popup", popup);
}

#[test]
fn approvals_selection_popup_snapshot() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    chat.config.notices.hide_full_access_warning = None;
    chat.open_approvals_popup();

    let popup = render_bottom_popup(&chat, 80);
    #[cfg(target_os = "windows")]
    insta::with_settings!({ snapshot_suffix => "windows" }, {
        assert_snapshot!("approvals_selection_popup", popup);
    });
    #[cfg(not(target_os = "windows"))]
    assert_snapshot!("approvals_selection_popup", popup);
}

#[test]
fn acp_resume_session_picker_snapshot() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    let sessions = vec![
        nori_acp::AcpSessionSummary {
            session_id: "session-abc".to_string(),
            cwd: PathBuf::from("/home/user/project"),
            title: Some("Refactor the parser".to_string()),
            updated_at: Some("2020-01-15T10:30:00Z".to_string()),
        },
        nori_acp::AcpSessionSummary {
            session_id: "session-def".to_string(),
            cwd: PathBuf::from("/home/user/other"),
            title: None,
            updated_at: None,
        },
    ];
    chat.show_acp_resume_session_picker(sessions);

    let popup = render_bottom_popup(&chat, 80);
    assert_snapshot!("acp_resume_session_picker", popup);
}

#[test]
fn approval_preset_actions_emit_a_single_atomic_app_event() {
    let (_chat, app_event_tx, mut rx, _op_rx) = make_chatwidget_manual_with_sender();
    let preset = builtin_approval_presets()
        .into_iter()
        .find(|preset| preset.id == "auto")
        .expect("agent preset");
    let actions = ChatWidget::approval_preset_actions(preset.approval, preset.sandbox.clone());

    assert_eq!(actions.len(), 1);
    actions[0](&app_event_tx);

    match rx.try_recv().expect("approval preset event") {
        AppEvent::ApplyApprovalPreset { approval, sandbox } => {
            assert_eq!(approval, preset.approval);
            assert_eq!(sandbox, preset.sandbox);
        }
        other => panic!("expected ApplyApprovalPreset event, got {other:?}"),
    }
    assert_matches!(rx.try_recv(), Err(TryRecvError::Empty));
}

#[test]
fn preset_matching_ignores_extra_writable_roots() {
    let preset = builtin_approval_presets()
        .into_iter()
        .find(|p| p.id == "auto")
        .expect("auto preset exists");
    let current_sandbox = SandboxPolicy::WorkspaceWrite {
        writable_roots: vec![PathBuf::from("C:\\extra")],
        network_access: false,
        exclude_tmpdir_env_var: false,
        exclude_slash_tmp: false,
    };

    assert!(
        ChatWidget::preset_matches_current(AskForApproval::OnRequest, &current_sandbox, &preset),
        "WorkspaceWrite with extra roots should still match the Agent preset"
    );
    assert!(
        !ChatWidget::preset_matches_current(AskForApproval::Never, &current_sandbox, &preset),
        "approval mismatch should prevent matching the preset"
    );
}

#[tokio::test]
async fn switch_skillset_with_name_intercepts_user_message() {
    let (mut chat, _rx, mut op_rx) = make_chatwidget_manual();

    // Submit "/switch-skillset foobar" as a user message.
    chat.submit_user_message("/switch-skillset foobar".to_string().into());

    // The message should NOT be sent to the model as a user input.
    // This proves the interception worked — the text was routed to the
    // skillset handler instead of being forwarded to the model.
    assert_matches!(op_rx.try_recv(), Err(TryRecvError::Empty));
}

#[test]
fn switch_skillset_without_name_is_not_intercepted() {
    let (mut chat, _rx, mut op_rx) = make_chatwidget_manual();

    // Submit "/switch-skillset " (trailing space, no actual name) as a user message.
    chat.submit_user_message("/switch-skillset ".to_string().into());

    // This should NOT be intercepted — it should be sent to the model as text.
    assert_matches!(op_rx.try_recv(), Ok(Op::UserInput { .. }));
}

#[test]
fn full_access_confirmation_popup_snapshot() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    let preset = builtin_approval_presets()
        .into_iter()
        .find(|preset| preset.id == "full-access")
        .expect("full access preset");
    chat.open_full_access_confirmation(preset);

    let popup = render_bottom_popup(&chat, 80);
    assert_snapshot!("full_access_confirmation_popup", popup);
}

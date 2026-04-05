use super::*;
use pretty_assertions::assert_eq;

#[cfg(target_os = "windows")]
#[test]
fn windows_auto_mode_prompt_requests_enabling_sandbox_feature() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    let preset = builtin_approval_presets()
        .into_iter()
        .find(|preset| preset.id == "auto")
        .expect("auto preset");
    chat.open_windows_sandbox_enable_prompt(preset);

    let popup = render_bottom_popup(&chat, 120);
    assert!(
        popup.contains("Agent mode on Windows uses an experimental sandbox"),
        "expected auto mode prompt to mention enabling the sandbox feature, popup: {popup}"
    );
}

#[cfg(target_os = "windows")]
#[test]
fn startup_prompts_for_windows_sandbox_when_agent_requested() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    set_windows_sandbox_enabled(false);
    chat.config.forced_auto_mode_downgraded_on_windows = true;

    chat.maybe_prompt_windows_sandbox_enable();

    let popup = render_bottom_popup(&chat, 120);
    assert!(
        popup.contains("Agent mode on Windows uses an experimental sandbox"),
        "expected startup prompt to explain sandbox: {popup}"
    );
    assert!(
        popup.contains("Enable experimental sandbox"),
        "expected startup prompt to offer enabling the sandbox: {popup}"
    );

    set_windows_sandbox_enabled(true);
}

#[test]
fn exec_history_extends_previous_when_consecutive() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    // 1) Start "ls -la" (List)
    let begin_ls = begin_exec(&mut chat, "call-ls", "ls -la");
    assert_snapshot!("exploring_step1_start_ls", active_blob(&chat));

    // 2) Finish "ls -la"
    end_exec(&mut chat, begin_ls, "", "", 0);
    assert_snapshot!("exploring_step2_finish_ls", active_blob(&chat));

    // 3) Start "cat foo.txt" (Read)
    let begin_cat_foo = begin_exec(&mut chat, "call-cat-foo", "cat foo.txt");
    assert_snapshot!("exploring_step3_start_cat_foo", active_blob(&chat));

    // 4) Complete "cat foo.txt"
    end_exec(&mut chat, begin_cat_foo, "hello from foo", "", 0);
    assert_snapshot!("exploring_step4_finish_cat_foo", active_blob(&chat));

    // 5) Start & complete "sed -n 100,200p foo.txt" (treated as Read of foo.txt)
    let begin_sed_range = begin_exec(&mut chat, "call-sed-range", "sed -n 100,200p foo.txt");
    end_exec(&mut chat, begin_sed_range, "chunk", "", 0);
    assert_snapshot!("exploring_step5_finish_sed_range", active_blob(&chat));

    // 6) Start & complete "cat bar.txt"
    let begin_cat_bar = begin_exec(&mut chat, "call-cat-bar", "cat bar.txt");
    end_exec(&mut chat, begin_cat_bar, "hello from bar", "", 0);
    assert_snapshot!("exploring_step6_finish_cat_bar", active_blob(&chat));
}

#[test]
fn user_shell_command_renders_output_not_exploring() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    let begin_ls = begin_exec_with_source(
        &mut chat,
        "user-shell-ls",
        "ls",
        ExecCommandSource::UserShell,
    );
    end_exec(&mut chat, begin_ls, "file1\nfile2\n", "", 0);

    let cells = drain_insert_history(&mut rx);
    assert_eq!(
        cells.len(),
        1,
        "expected a single history cell for the user command"
    );
    let blob = lines_to_single_string(cells.first().unwrap());
    assert_snapshot!("user_shell_ls_output", blob);
}

#[test]
fn disabled_slash_command_while_task_running_snapshot() {
    // Build a chat widget and simulate an active task
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.bottom_pane.set_task_running(true);

    // Dispatch a command that is unavailable while a task runs (e.g., /model)
    chat.dispatch_command(SlashCommand::Model);

    // Drain history and snapshot the rendered error line(s)
    let cells = drain_insert_history(&mut rx);
    assert!(
        !cells.is_empty(),
        "expected an error message history cell to be emitted",
    );
    let blob = lines_to_single_string(cells.last().unwrap());
    assert_snapshot!(blob);
}

#[test]
fn approval_modal_exec_snapshot() {
    // Build a chat widget with manual channels to avoid spawning the agent.
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    // Ensure policy allows surfacing approvals explicitly (not strictly required for direct event).
    chat.config.approval_policy = AskForApproval::OnRequest;
    // Inject an exec approval request to display the approval modal.
    let ev = ExecApprovalRequestEvent {
        call_id: "call-approve-cmd".into(),
        turn_id: "turn-approve-cmd".into(),
        command: vec!["bash".into(), "-lc".into(), "echo hello world".into()],
        cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        reason: Some(
            "this is a test reason such as one that would be produced by the model".into(),
        ),
        risk: None,
        parsed_cmd: vec![],
    };
    chat.handle_codex_event(Event {
        id: "sub-approve".into(),
        msg: EventMsg::ExecApprovalRequest(ev),
    });
    // Render to a fixed-size test terminal and snapshot.
    // Call desired_height first and use that exact height for rendering.
    let height = chat.desired_height(80);
    let mut terminal =
        crate::custom_terminal::Terminal::with_options(VT100Backend::new(80, height))
            .expect("create terminal");
    let viewport = Rect::new(0, 0, 80, height);
    terminal.set_viewport_area(viewport);

    terminal
        .draw(|f| chat.render(f.area(), f.buffer_mut()))
        .expect("draw approval modal");
    assert!(
        terminal
            .backend()
            .vt100()
            .screen()
            .contents()
            .contains("echo hello world")
    );
    assert_snapshot!(
        "approval_modal_exec",
        terminal.backend().vt100().screen().contents()
    );
}

#[test]
fn approval_modal_exec_without_reason_snapshot() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    chat.config.approval_policy = AskForApproval::OnRequest;

    let ev = ExecApprovalRequestEvent {
        call_id: "call-approve-cmd-noreason".into(),
        turn_id: "turn-approve-cmd-noreason".into(),
        command: vec!["bash".into(), "-lc".into(), "echo hello world".into()],
        cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        reason: None,
        risk: None,
        parsed_cmd: vec![],
    };
    chat.handle_codex_event(Event {
        id: "sub-approve-noreason".into(),
        msg: EventMsg::ExecApprovalRequest(ev),
    });

    let height = chat.desired_height(80);
    let mut terminal =
        ratatui::Terminal::new(VT100Backend::new(80, height)).expect("create terminal");
    terminal.set_viewport_area(Rect::new(0, 0, 80, height));
    terminal
        .draw(|f| chat.render(f.area(), f.buffer_mut()))
        .expect("draw approval modal (no reason)");
    assert_snapshot!(
        "approval_modal_exec_no_reason",
        terminal.backend().vt100().screen().contents()
    );
}

#[test]
fn approval_modal_patch_snapshot() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    chat.config.approval_policy = AskForApproval::OnRequest;

    // Build a small changeset and a reason/grant_root to exercise the prompt text.
    let mut changes = HashMap::new();
    changes.insert(
        PathBuf::from("README.md"),
        FileChange::Add {
            content: "hello\nworld\n".into(),
        },
    );
    let ev = ApplyPatchApprovalRequestEvent {
        call_id: "call-approve-patch".into(),
        turn_id: "turn-approve-patch".into(),
        changes,
        reason: Some("The model wants to apply changes".into()),
        grant_root: Some(PathBuf::from("/tmp")),
    };
    chat.handle_codex_event(Event {
        id: "sub-approve-patch".into(),
        msg: EventMsg::ApplyPatchApprovalRequest(ev),
    });

    // Render at the widget's desired height and snapshot.
    let height = chat.desired_height(80);
    let mut terminal =
        ratatui::Terminal::new(VT100Backend::new(80, height)).expect("create terminal");
    terminal.set_viewport_area(Rect::new(0, 0, 80, height));
    terminal
        .draw(|f| chat.render(f.area(), f.buffer_mut()))
        .expect("draw patch approval modal");
    assert_snapshot!(
        "approval_modal_patch",
        terminal.backend().vt100().screen().contents()
    );
}

#[test]
fn normalized_answer_message_delta_starts_streaming_controller() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    chat.handle_client_event(nori_protocol::ClientEvent::MessageDelta(
        nori_protocol::MessageDelta {
            stream: nori_protocol::MessageStream::Answer,
            delta: "Here is the normalized answer.\n".into(),
        },
    ));

    assert!(
        chat.stream_controller.is_some(),
        "normalized answer deltas should start the live streaming controller"
    );
}

#[test]
fn normalized_reasoning_message_delta_updates_status_header() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    chat.on_task_started();

    chat.handle_client_event(nori_protocol::ClientEvent::MessageDelta(
        nori_protocol::MessageDelta {
            stream: nori_protocol::MessageStream::Reasoning,
            delta: "**Analyzing** the live ACP flow".into(),
        },
    ));

    assert_eq!(chat.current_status_header, "Analyzing");
}

#[test]
fn normalized_turn_started_sets_task_running() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    chat.handle_client_event(nori_protocol::ClientEvent::TurnLifecycle(
        nori_protocol::TurnLifecycle::Started,
    ));

    assert!(chat.bottom_pane.is_task_running());
}

#[test]
fn normalized_turn_aborted_restores_queued_messages_into_composer() {
    let (mut chat, mut rx, mut op_rx) = make_chatwidget_manual();

    chat.bottom_pane.set_task_running(true);
    chat.queued_user_messages
        .push_back(UserMessage::from("first queued".to_string()));
    chat.queued_user_messages
        .push_back(UserMessage::from("second queued".to_string()));
    chat.refresh_queued_user_messages();

    chat.handle_client_event(nori_protocol::ClientEvent::TurnLifecycle(
        nori_protocol::TurnLifecycle::Aborted {
            reason: nori_protocol::TurnAbortReason::Interrupted,
        },
    ));

    assert_eq!(
        chat.bottom_pane.composer_text(),
        "first queued\nsecond queued"
    );
    assert!(chat.queued_user_messages.is_empty());
    assert!(
        op_rx.try_recv().is_err(),
        "unexpected outbound op after interrupt"
    );

    let _ = drain_insert_history(&mut rx);
}

#[test]
fn replay_entry_user_and_assistant_render_history() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.handle_client_event(nori_protocol::ClientEvent::ReplayEntry(
        nori_protocol::ReplayEntry::UserMessage {
            text: "Resume the session".into(),
        },
    ));
    chat.handle_client_event(nori_protocol::ClientEvent::ReplayEntry(
        nori_protocol::ReplayEntry::AssistantMessage {
            text: "Resuming now.".into(),
        },
    ));

    let history = drain_insert_history(&mut rx);
    assert!(
        history.len() >= 2,
        "replay entries should produce visible history output"
    );
}

#[test]
fn approval_modal_patch_from_client_event_snapshot() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    chat.config.approval_policy = AskForApproval::OnRequest;

    chat.handle_client_event(nori_protocol::ClientEvent::ApprovalRequest(
        nori_protocol::ApprovalRequest {
            call_id: "call-approve-patch-client-event".into(),
            title: "Write README.md".into(),
            kind: nori_protocol::ToolKind::Edit,
            options: vec![],
            subject: nori_protocol::ApprovalSubject::ToolSnapshot(nori_protocol::ToolSnapshot {
                call_id: "call-approve-patch-client-event".into(),
                title: "Write README.md".into(),
                kind: nori_protocol::ToolKind::Edit,
                phase: nori_protocol::ToolPhase::PendingApproval,
                locations: vec![],
                invocation: Some(nori_protocol::Invocation::FileChanges {
                    changes: vec![nori_protocol::FileChange {
                        path: PathBuf::from("README.md"),
                        old_text: None,
                        new_text: "hello\nworld\n".into(),
                    }],
                }),
                artifacts: vec![nori_protocol::Artifact::Diff(nori_protocol::FileChange {
                    path: PathBuf::from("README.md"),
                    old_text: None,
                    new_text: "hello\nworld\n".into(),
                })],
                raw_input: None,
                raw_output: None,
                owner_request_id: None,
            }),
        },
    ));

    let height = chat.desired_height(80);
    let mut terminal =
        ratatui::Terminal::new(VT100Backend::new(80, height)).expect("create terminal");
    terminal.set_viewport_area(Rect::new(0, 0, 80, height));
    terminal
        .draw(|f| chat.render(f.area(), f.buffer_mut()))
        .expect("draw patch approval modal");
    assert_snapshot!(
        "approval_modal_patch_from_client_event",
        terminal.backend().vt100().screen().contents()
    );
}

#[test]
fn approval_modal_exec_from_client_event() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    chat.config.approval_policy = AskForApproval::OnRequest;

    chat.handle_client_event(nori_protocol::ClientEvent::ApprovalRequest(
        nori_protocol::ApprovalRequest {
            call_id: "call-approve-exec-client-event".into(),
            title: "Terminal".into(),
            kind: nori_protocol::ToolKind::Execute,
            options: vec![],
            subject: nori_protocol::ApprovalSubject::ToolSnapshot(nori_protocol::ToolSnapshot {
                call_id: "call-approve-exec-client-event".into(),
                title: "Terminal".into(),
                kind: nori_protocol::ToolKind::Execute,
                phase: nori_protocol::ToolPhase::PendingApproval,
                locations: vec![],
                invocation: Some(nori_protocol::Invocation::Command {
                    command: "git status".into(),
                }),
                artifacts: vec![],
                raw_input: Some(serde_json::json!({"command": "git status"})),
                raw_output: None,
                owner_request_id: None,
            }),
        },
    ));

    let height = chat.desired_height(80);
    let mut terminal =
        ratatui::Terminal::new(VT100Backend::new(80, height)).expect("create terminal");
    terminal.set_viewport_area(Rect::new(0, 0, 80, height));
    terminal
        .draw(|f| chat.render(f.area(), f.buffer_mut()))
        .expect("draw exec approval modal");
    let contents = terminal.backend().vt100().screen().contents();
    assert!(
        contents.contains("git status"),
        "expected exec approval modal: {contents:?}"
    );
}

#[test]
fn interrupt_restores_queued_messages_into_composer() {
    let (mut chat, mut rx, mut op_rx) = make_chatwidget_manual();

    // Simulate a running task to enable queuing of user inputs.
    chat.bottom_pane.set_task_running(true);

    // Queue two user messages while the task is running.
    chat.queued_user_messages
        .push_back(UserMessage::from("first queued".to_string()));
    chat.queued_user_messages
        .push_back(UserMessage::from("second queued".to_string()));
    chat.refresh_queued_user_messages();

    // Deliver a TurnAborted event with Interrupted reason (as if Esc was pressed).
    chat.handle_codex_event(Event {
        id: "turn-1".into(),
        msg: EventMsg::TurnAborted(codex_core::protocol::TurnAbortedEvent {
            reason: TurnAbortReason::Interrupted,
        }),
    });

    // Composer should now contain the queued messages joined by newlines, in order.
    assert_eq!(
        chat.bottom_pane.composer_text(),
        "first queued\nsecond queued"
    );

    // Queue should be cleared and no new user input should have been auto-submitted.
    assert!(chat.queued_user_messages.is_empty());
    assert!(
        op_rx.try_recv().is_err(),
        "unexpected outbound op after interrupt"
    );

    // Drain rx to avoid unused warnings.
    let _ = drain_insert_history(&mut rx);
}

#[test]
fn interrupt_prepends_queued_messages_before_existing_composer_text() {
    let (mut chat, mut rx, mut op_rx) = make_chatwidget_manual();

    chat.bottom_pane.set_task_running(true);
    chat.bottom_pane
        .set_composer_text("current draft".to_string());

    chat.queued_user_messages
        .push_back(UserMessage::from("first queued".to_string()));
    chat.queued_user_messages
        .push_back(UserMessage::from("second queued".to_string()));
    chat.refresh_queued_user_messages();

    chat.handle_codex_event(Event {
        id: "turn-1".into(),
        msg: EventMsg::TurnAborted(codex_core::protocol::TurnAbortedEvent {
            reason: TurnAbortReason::Interrupted,
        }),
    });

    assert_eq!(
        chat.bottom_pane.composer_text(),
        "first queued\nsecond queued\ncurrent draft"
    );
    assert!(chat.queued_user_messages.is_empty());
    assert!(
        op_rx.try_recv().is_err(),
        "unexpected outbound op after interrupt"
    );

    let _ = drain_insert_history(&mut rx);
}

#[test]
fn ui_snapshots_small_heights_idle() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    let (chat, _rx, _op_rx) = make_chatwidget_manual();
    for h in [1u16, 2, 3] {
        let name = format!("chat_small_idle_h{h}");
        let mut terminal = Terminal::new(TestBackend::new(40, h)).expect("create terminal");
        terminal
            .draw(|f| chat.render(f.area(), f.buffer_mut()))
            .expect("draw chat idle");
        assert_snapshot!(name, terminal.backend());
    }
}

#[test]
fn ui_snapshots_small_heights_task_running() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    // Activate status line
    chat.handle_codex_event(Event {
        id: "task-1".into(),
        msg: EventMsg::TaskStarted(TaskStartedEvent {
            model_context_window: None,
        }),
    });
    chat.handle_codex_event(Event {
        id: "task-1".into(),
        msg: EventMsg::AgentReasoningDelta(AgentReasoningDeltaEvent {
            delta: "**Thinking**".into(),
        }),
    });
    for h in [1u16, 2, 3] {
        let name = format!("chat_small_running_h{h}");
        let mut terminal = Terminal::new(TestBackend::new(40, h)).expect("create terminal");
        terminal
            .draw(|f| chat.render(f.area(), f.buffer_mut()))
            .expect("draw chat running");
        assert_snapshot!(name, terminal.backend());
    }
}

#[test]
fn status_widget_and_approval_modal_snapshot() {
    use codex_core::protocol::ExecApprovalRequestEvent;

    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    // Begin a running task so the status indicator would be active.
    chat.handle_codex_event(Event {
        id: "task-1".into(),
        msg: EventMsg::TaskStarted(TaskStartedEvent {
            model_context_window: None,
        }),
    });
    // Provide a deterministic header for the status line.
    chat.handle_codex_event(Event {
        id: "task-1".into(),
        msg: EventMsg::AgentReasoningDelta(AgentReasoningDeltaEvent {
            delta: "**Analyzing**".into(),
        }),
    });

    // Now show an approval modal (e.g. exec approval).
    let ev = ExecApprovalRequestEvent {
        call_id: "call-approve-exec".into(),
        turn_id: "turn-approve-exec".into(),
        command: vec!["echo".into(), "hello world".into()],
        cwd: PathBuf::from("/tmp"),
        reason: Some(
            "this is a test reason such as one that would be produced by the model".into(),
        ),
        risk: None,
        parsed_cmd: vec![],
    };
    chat.handle_codex_event(Event {
        id: "sub-approve-exec".into(),
        msg: EventMsg::ExecApprovalRequest(ev),
    });

    // Render at the widget's desired height and snapshot.
    let height = chat.desired_height(80);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, height))
        .expect("create terminal");
    terminal
        .draw(|f| chat.render(f.area(), f.buffer_mut()))
        .expect("draw status + approval modal");
    assert_snapshot!("status_widget_and_approval_modal", terminal.backend());
}

#[test]
fn status_widget_active_snapshot() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    // Activate the status indicator by simulating a task start.
    chat.handle_codex_event(Event {
        id: "task-1".into(),
        msg: EventMsg::TaskStarted(TaskStartedEvent {
            model_context_window: None,
        }),
    });
    // Provide a deterministic header via a bold reasoning chunk.
    chat.handle_codex_event(Event {
        id: "task-1".into(),
        msg: EventMsg::AgentReasoningDelta(AgentReasoningDeltaEvent {
            delta: "**Analyzing**".into(),
        }),
    });
    // Render and snapshot.
    let height = chat.desired_height(80);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, height))
        .expect("create terminal");
    terminal
        .draw(|f| chat.render(f.area(), f.buffer_mut()))
        .expect("draw status widget");
    assert_snapshot!("status_widget_active", terminal.backend());
}

#[test]
fn background_event_updates_status_header() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.handle_codex_event(Event {
        id: "bg-1".into(),
        msg: EventMsg::BackgroundEvent(BackgroundEventEvent {
            message: "Waiting for `vim`".to_string(),
        }),
    });

    assert!(chat.bottom_pane.status_indicator_visible());
    assert_eq!(chat.current_status_header, "Waiting for `vim`");
    assert!(drain_insert_history(&mut rx).is_empty());
}

#[test]
fn apply_patch_events_emit_history_cells() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    // 1) Approval request -> proposed patch summary cell
    let mut changes = HashMap::new();
    changes.insert(
        PathBuf::from("foo.txt"),
        FileChange::Add {
            content: "hello\n".to_string(),
        },
    );
    let ev = ApplyPatchApprovalRequestEvent {
        call_id: "c1".into(),
        turn_id: "turn-c1".into(),
        changes,
        reason: None,
        grant_root: None,
    };
    chat.handle_codex_event(Event {
        id: "s1".into(),
        msg: EventMsg::ApplyPatchApprovalRequest(ev),
    });
    let cells = drain_insert_history(&mut rx);
    assert!(
        cells.is_empty(),
        "expected approval request to surface via modal without emitting history cells"
    );

    let area = Rect::new(0, 0, 80, chat.desired_height(80));
    let mut buf = ratatui::buffer::Buffer::empty(area);
    chat.render(area, &mut buf);
    let mut saw_summary = false;
    for y in 0..area.height {
        let mut row = String::new();
        for x in 0..area.width {
            row.push(buf[(x, y)].symbol().chars().next().unwrap_or(' '));
        }
        if row.contains("foo.txt (+1 -0)") {
            saw_summary = true;
            break;
        }
    }
    assert!(saw_summary, "expected approval modal to show diff summary");

    // 2) Begin apply -> per-file apply block cell (no global header)
    let mut changes2 = HashMap::new();
    changes2.insert(
        PathBuf::from("foo.txt"),
        FileChange::Add {
            content: "hello\n".to_string(),
        },
    );
    let begin = PatchApplyBeginEvent {
        call_id: "c1".into(),
        turn_id: "turn-c1".into(),
        auto_approved: true,
        changes: changes2,
    };
    chat.handle_codex_event(Event {
        id: "s1".into(),
        msg: EventMsg::PatchApplyBegin(begin),
    });
    let cells = drain_insert_history(&mut rx);
    assert!(!cells.is_empty(), "expected apply block cell to be sent");
    let blob = lines_to_single_string(cells.last().unwrap());
    assert!(
        blob.contains("Added foo.txt") || blob.contains("Edited foo.txt"),
        "expected single-file header with filename (Added/Edited): {blob:?}"
    );

    // 3) End apply success -> success cell
    let mut end_changes = HashMap::new();
    end_changes.insert(
        PathBuf::from("foo.txt"),
        FileChange::Add {
            content: "hello\n".to_string(),
        },
    );
    let end = PatchApplyEndEvent {
        call_id: "c1".into(),
        turn_id: "turn-c1".into(),
        stdout: "ok\n".into(),
        stderr: String::new(),
        success: true,
        changes: end_changes,
    };
    chat.handle_codex_event(Event {
        id: "s1".into(),
        msg: EventMsg::PatchApplyEnd(end),
    });
    let cells = drain_insert_history(&mut rx);
    assert!(
        cells.is_empty(),
        "no success cell should be emitted anymore"
    );
}

#[test]
fn apply_patch_manual_approval_adjusts_header() {
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
            reason: None,
            grant_root: None,
        }),
    });
    drain_insert_history(&mut rx);

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

    let cells = drain_insert_history(&mut rx);
    assert!(!cells.is_empty(), "expected apply block cell to be sent");
    let blob = lines_to_single_string(cells.last().unwrap());
    assert!(
        blob.contains("Added foo.txt") || blob.contains("Edited foo.txt"),
        "expected apply summary header for foo.txt: {blob:?}"
    );
}

#[test]
fn completed_edit_tool_snapshot_renders_patch_history_cell() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(
        nori_protocol::ToolSnapshot {
            call_id: "call-edit-complete".into(),
            title: "Write README.md".into(),
            kind: nori_protocol::ToolKind::Edit,
            phase: nori_protocol::ToolPhase::Completed,
            locations: vec![nori_protocol::ToolLocation {
                path: PathBuf::from("README.md"),
                line: None,
            }],
            invocation: Some(nori_protocol::Invocation::FileChanges {
                changes: vec![nori_protocol::FileChange {
                    path: PathBuf::from("README.md"),
                    old_text: None,
                    new_text: "hello\nworld\n".into(),
                }],
            }),
            artifacts: vec![nori_protocol::Artifact::Diff(nori_protocol::FileChange {
                path: PathBuf::from("README.md"),
                old_text: None,
                new_text: "hello\nworld\n".into(),
            })],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        },
    ));

    let cells = drain_insert_history(&mut rx);
    assert_eq!(cells.len(), 1, "expected one edit history cell");
    let blob = lines_to_single_string(cells.first().unwrap());
    assert_snapshot!("completed_edit_tool_snapshot", blob);
}

#[test]
fn completed_delete_tool_snapshot_renders_patch_history_cell() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(
        nori_protocol::ToolSnapshot {
            call_id: "call-delete-complete".into(),
            title: "Delete README.md".into(),
            kind: nori_protocol::ToolKind::Delete,
            phase: nori_protocol::ToolPhase::Completed,
            locations: vec![nori_protocol::ToolLocation {
                path: PathBuf::from("README.md"),
                line: None,
            }],
            invocation: Some(nori_protocol::Invocation::FileOperations {
                operations: vec![nori_protocol::FileOperation::Delete {
                    path: PathBuf::from("README.md"),
                    old_text: Some("hello\nworld\n".into()),
                }],
            }),
            artifacts: vec![],
            raw_input: Some(serde_json::json!({
                "path": "README.md",
                "content": "hello\nworld\n",
            })),
            raw_output: None,
            owner_request_id: None,
        },
    ));

    let cells = drain_insert_history(&mut rx);
    assert_eq!(cells.len(), 1, "expected one patch history cell");
    let blob = lines_to_single_string(cells.first().unwrap());
    assert!(
        blob.contains("Deleted README.md"),
        "expected delete summary in patch cell: {blob:?}"
    );
}

#[test]
fn completed_move_tool_snapshot_renders_patch_history_cell() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(
        nori_protocol::ToolSnapshot {
            call_id: "call-move-complete".into(),
            title: "Move README.md".into(),
            kind: nori_protocol::ToolKind::Move,
            phase: nori_protocol::ToolPhase::Completed,
            locations: vec![nori_protocol::ToolLocation {
                path: PathBuf::from("README.md"),
                line: None,
            }],
            invocation: Some(nori_protocol::Invocation::FileOperations {
                operations: vec![nori_protocol::FileOperation::Move {
                    from_path: PathBuf::from("README.md"),
                    to_path: PathBuf::from("docs/README.md"),
                    old_text: Some("hello\nworld\n".into()),
                    new_text: Some("hello\nworld\n".into()),
                }],
            }),
            artifacts: vec![],
            raw_input: Some(serde_json::json!({
                "from": "README.md",
                "to": "docs/README.md",
            })),
            raw_output: None,
            owner_request_id: None,
        },
    ));

    let cells = drain_insert_history(&mut rx);
    assert_eq!(cells.len(), 1, "expected one patch history cell");
    let blob = lines_to_single_string(cells.first().unwrap());
    assert!(
        blob.contains("README.md") && blob.contains("docs/README.md"),
        "expected move summary in patch cell: {blob:?}"
    );
}

#[test]
fn completed_execute_tool_snapshot_renders_exec_history_cell() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(
        nori_protocol::ToolSnapshot {
            call_id: "call-exec-complete".into(),
            title: "Terminal".into(),
            kind: nori_protocol::ToolKind::Execute,
            phase: nori_protocol::ToolPhase::Completed,
            locations: vec![],
            invocation: Some(nori_protocol::Invocation::Command {
                command: "git status".into(),
            }),
            artifacts: vec![nori_protocol::Artifact::Text {
                text: "On branch main\n".into(),
            }],
            raw_input: Some(serde_json::json!({"command": "git status"})),
            raw_output: Some(serde_json::json!({"stdout": "On branch main\n"})),
            owner_request_id: None,
        },
    ));

    let cells = drain_insert_history(&mut rx);
    assert_eq!(cells.len(), 1, "expected one exec history cell");
    let blob = lines_to_single_string(cells.first().unwrap());
    assert!(
        blob.contains("git status"),
        "expected command in history cell: {blob:?}"
    );
    assert!(
        blob.contains("On branch main"),
        "expected output in history cell: {blob:?}"
    );
}

#[test]
fn pending_execute_tool_snapshot_renders_running_exec_cell() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(
        nori_protocol::ToolSnapshot {
            call_id: "call-exec-pending".into(),
            title: "Terminal".into(),
            kind: nori_protocol::ToolKind::Execute,
            phase: nori_protocol::ToolPhase::Pending,
            locations: vec![],
            invocation: Some(nori_protocol::Invocation::Command {
                command: "git status".into(),
            }),
            artifacts: vec![],
            raw_input: Some(serde_json::json!({"command": "git status"})),
            raw_output: None,
            owner_request_id: None,
        },
    ));

    let blob = active_blob(&chat);
    assert!(
        blob.contains("Running"),
        "expected running exec cell from normalized pending snapshot: {blob:?}"
    );
    assert!(
        blob.contains("git status"),
        "expected command in running exec cell: {blob:?}"
    );
}

#[test]
fn completed_execute_tool_snapshot_is_not_deferred_during_streaming() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.on_task_started();
    drain_insert_history(&mut rx);

    chat.on_agent_message_delta("Sure, let me check...\n".to_string());
    chat.on_commit_tick();
    let first_text = drain_insert_history(&mut rx);
    assert!(
        !first_text.is_empty(),
        "first text block should have been committed to history"
    );

    chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(
        nori_protocol::ToolSnapshot {
            call_id: "call-exec-complete".into(),
            title: "Terminal".into(),
            kind: nori_protocol::ToolKind::Execute,
            phase: nori_protocol::ToolPhase::Completed,
            locations: vec![],
            invocation: Some(nori_protocol::Invocation::Command {
                command: "git status".into(),
            }),
            artifacts: vec![nori_protocol::Artifact::Text {
                text: "On branch main\n".into(),
            }],
            raw_input: Some(serde_json::json!({"command": "git status"})),
            raw_output: Some(serde_json::json!({"stdout": "On branch main\n"})),
            owner_request_id: None,
        },
    ));

    assert!(
        chat.interrupts.is_empty(),
        "completed execute client snapshot should not be deferred"
    );

    chat.on_agent_message_delta("Done!\n".to_string());
    chat.on_commit_tick();
    chat.on_task_complete(None);

    let cells = drain_insert_history(&mut rx);
    let combined: Vec<String> = cells.iter().map(|c| lines_to_single_string(c)).collect();
    let full = combined.join("");

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
        "tool call should appear before second text block, but tool_pos={} >= done_pos={}\nfull output: {full:?}",
        tool_pos.unwrap(),
        done_pos.unwrap(),
    );
}

#[test]
fn completed_read_tool_snapshot_renders_exploring_history_cell() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(
        nori_protocol::ToolSnapshot {
            call_id: "call-read-complete".into(),
            title: "Read Cargo.toml".into(),
            kind: nori_protocol::ToolKind::Read,
            phase: nori_protocol::ToolPhase::Completed,
            locations: vec![],
            invocation: Some(nori_protocol::Invocation::Read {
                path: PathBuf::from("Cargo.toml"),
            }),
            artifacts: vec![nori_protocol::Artifact::Text {
                text: "[package]\nname = \"nori\"\n".into(),
            }],
            raw_input: Some(serde_json::json!({"path": "Cargo.toml"})),
            raw_output: Some(serde_json::json!({"stdout": "[package]\nname = \"nori\"\n"})),
            owner_request_id: None,
        },
    ));

    let blob = active_blob(&chat);
    assert!(
        blob.contains("Explored"),
        "expected exploring summary header: {blob:?}"
    );
    assert!(
        blob.contains("Cargo.toml"),
        "expected read target in exploring cell: {blob:?}"
    );
}

#[test]
fn completed_search_tool_snapshot_renders_exploring_history_cell() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(
        nori_protocol::ToolSnapshot {
            call_id: "call-search-complete".into(),
            title: "Search src".into(),
            kind: nori_protocol::ToolKind::Search,
            phase: nori_protocol::ToolPhase::Completed,
            locations: vec![],
            invocation: Some(nori_protocol::Invocation::Search {
                query: Some("TODO".into()),
                path: Some(PathBuf::from("src")),
            }),
            artifacts: vec![nori_protocol::Artifact::Text {
                text: "src/main.rs:12:// TODO\n".into(),
            }],
            raw_input: Some(serde_json::json!({"pattern": "TODO", "path": "src"})),
            raw_output: Some(serde_json::json!({"stdout": "src/main.rs:12:// TODO\n"})),
            owner_request_id: None,
        },
    ));

    let blob = active_blob(&chat);
    assert!(
        blob.contains("Explored"),
        "expected exploring summary header: {blob:?}"
    );
    assert!(
        blob.contains("TODO"),
        "expected search query in exploring cell: {blob:?}"
    );
    assert!(
        blob.contains("src"),
        "expected search path in exploring cell: {blob:?}"
    );
}

#[test]
fn completed_list_files_tool_snapshot_renders_exploring_history_cell() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(
        nori_protocol::ToolSnapshot {
            call_id: "call-list-complete".into(),
            title: "List src".into(),
            kind: nori_protocol::ToolKind::Search,
            phase: nori_protocol::ToolPhase::Completed,
            locations: vec![],
            invocation: Some(nori_protocol::Invocation::ListFiles {
                path: Some(PathBuf::from("src")),
            }),
            artifacts: vec![nori_protocol::Artifact::Text {
                text: "src/main.rs\nsrc/lib.rs\n".into(),
            }],
            raw_input: Some(serde_json::json!({"path": "src"})),
            raw_output: Some(serde_json::json!({"stdout": "src/main.rs\nsrc/lib.rs\n"})),
            owner_request_id: None,
        },
    ));

    let blob = active_blob(&chat);
    assert!(
        blob.contains("Explored"),
        "expected exploring summary header: {blob:?}"
    );
    assert!(
        blob.contains("src"),
        "expected list target in exploring cell: {blob:?}"
    );
}

#[test]
fn completed_generic_execute_tool_snapshot_renders_exec_history_cell() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(
        nori_protocol::ToolSnapshot {
            call_id: "toolu_generic_test_001".into(),
            title: "Terminal".into(),
            kind: nori_protocol::ToolKind::Execute,
            phase: nori_protocol::ToolPhase::Completed,
            locations: vec![],
            invocation: None,
            artifacts: vec![nori_protocol::Artifact::Text {
                text: "command output here".into(),
            }],
            raw_input: None,
            raw_output: Some(serde_json::json!({
                "exit_code": 0,
                "stdout": "command output here",
            })),
            owner_request_id: None,
        },
    ));

    let cells = drain_insert_history(&mut rx);
    assert_eq!(cells.len(), 1, "expected one exec history cell");
    let blob = lines_to_single_string(cells.first().unwrap());
    assert!(
        blob.contains("Ran Terminal"),
        "expected resolved generic tool title in history cell: {blob:?}"
    );
    assert!(
        !blob.contains("toolu_generic_test_001"),
        "raw call id should not be rendered in history cell: {blob:?}"
    );
}

#[test]
fn completed_fetch_tool_snapshot_renders_exec_history_cell() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(
        nori_protocol::ToolSnapshot {
            call_id: "call-fetch-complete".into(),
            title: "Fetch".into(),
            kind: nori_protocol::ToolKind::Fetch,
            phase: nori_protocol::ToolPhase::Completed,
            locations: vec![],
            invocation: Some(nori_protocol::Invocation::Tool {
                tool_name: "Fetch".into(),
                input: Some(serde_json::json!({
                    "url": "https://example.com",
                })),
            }),
            artifacts: vec![nori_protocol::Artifact::Text {
                text: "ok\n".into(),
            }],
            raw_input: Some(serde_json::json!({
                "url": "https://example.com",
            })),
            raw_output: Some(serde_json::json!({
                "stdout": "ok\n",
            })),
            owner_request_id: None,
        },
    ));

    let cells = drain_insert_history(&mut rx);
    assert_eq!(cells.len(), 1, "expected one exec history cell");
    let blob = lines_to_single_string(cells.first().unwrap());
    assert!(
        blob.contains("Fetch"),
        "expected tool title in history cell: {blob:?}"
    );
    assert!(
        blob.contains("ok"),
        "expected output in history cell: {blob:?}"
    );
}

// --- Spec 05: In-Progress Edit/Delete/Move Rendering ---

#[test]
fn in_progress_edit_renders_active_client_tool_cell() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(
        nori_protocol::ToolSnapshot {
            call_id: "call-edit-progress".into(),
            title: "Write README.md".into(),
            kind: nori_protocol::ToolKind::Edit,
            phase: nori_protocol::ToolPhase::InProgress,
            locations: vec![nori_protocol::ToolLocation {
                path: PathBuf::from("README.md"),
                line: None,
            }],
            invocation: None,
            artifacts: vec![],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        },
    ));

    // Should NOT produce any flushed history cells
    let cells = drain_insert_history(&mut rx);
    assert!(
        cells.is_empty(),
        "In-progress edit should not flush to history, got {cells:?}",
    );

    // Should have an active cell (the spinner)
    let blob = active_blob(&chat);
    assert!(
        blob.contains("Editing README.md"),
        "Active cell should show semantic edit header, got: {blob:?}"
    );
}

#[test]
fn completed_edit_after_in_progress_replaces_spinner_with_patch() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    // First: in-progress edit creates spinner
    chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(
        nori_protocol::ToolSnapshot {
            call_id: "call-edit-lifecycle".into(),
            title: "Write README.md".into(),
            kind: nori_protocol::ToolKind::Edit,
            phase: nori_protocol::ToolPhase::InProgress,
            locations: vec![nori_protocol::ToolLocation {
                path: PathBuf::from("README.md"),
                line: None,
            }],
            invocation: None,
            artifacts: vec![],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        },
    ));

    // Drain any events from in-progress
    let _ = drain_insert_history(&mut rx);

    // Then: completed edit with diff arrives
    chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(
        nori_protocol::ToolSnapshot {
            call_id: "call-edit-lifecycle".into(),
            title: "Write README.md".into(),
            kind: nori_protocol::ToolKind::Edit,
            phase: nori_protocol::ToolPhase::Completed,
            locations: vec![],
            invocation: Some(nori_protocol::Invocation::FileChanges {
                changes: vec![nori_protocol::FileChange {
                    path: PathBuf::from("README.md"),
                    old_text: None,
                    new_text: "hello\nworld\n".into(),
                }],
            }),
            artifacts: vec![nori_protocol::Artifact::Diff(nori_protocol::FileChange {
                path: PathBuf::from("README.md"),
                old_text: None,
                new_text: "hello\nworld\n".into(),
            })],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        },
    ));

    let cells = drain_insert_history(&mut rx);
    // Should have exactly 1 cell (the PatchHistoryCell), NOT 2 (spinner + patch)
    assert_eq!(
        cells.len(),
        1,
        "Expected only one cell (patch), not spinner+patch, got {} cells",
        cells.len()
    );
    let blob = lines_to_single_string(cells.first().unwrap());
    assert!(
        blob.contains("README.md"),
        "Patch cell should show filename, got: {blob:?}"
    );
}

#[test]
fn in_progress_delete_renders_active_cell() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(
        nori_protocol::ToolSnapshot {
            call_id: "call-delete-progress".into(),
            title: "Delete temp.txt".into(),
            kind: nori_protocol::ToolKind::Delete,
            phase: nori_protocol::ToolPhase::InProgress,
            locations: vec![nori_protocol::ToolLocation {
                path: PathBuf::from("temp.txt"),
                line: None,
            }],
            invocation: None,
            artifacts: vec![],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        },
    ));

    let cells = drain_insert_history(&mut rx);
    assert!(
        cells.is_empty(),
        "In-progress delete should not flush to history"
    );

    let blob = active_blob(&chat);
    assert!(
        blob.contains("Deleting temp.txt"),
        "Active cell should show semantic delete header, got: {blob:?}"
    );
}

// --- Spec 02: Exploring Cell Grouping ---

#[test]
fn consecutive_read_snapshots_merge_into_single_exploring_cell() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    // Send two read snapshots
    chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(
        nori_protocol::ToolSnapshot {
            call_id: "call-r1".into(),
            title: "Read file1.rs".into(),
            kind: nori_protocol::ToolKind::Read,
            phase: nori_protocol::ToolPhase::Completed,
            locations: vec![],
            invocation: Some(nori_protocol::Invocation::Read {
                path: PathBuf::from("file1.rs"),
            }),
            artifacts: vec![],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        },
    ));

    chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(
        nori_protocol::ToolSnapshot {
            call_id: "call-r2".into(),
            title: "Read file2.rs".into(),
            kind: nori_protocol::ToolKind::Read,
            phase: nori_protocol::ToolPhase::Completed,
            locations: vec![],
            invocation: Some(nori_protocol::Invocation::Read {
                path: PathBuf::from("file2.rs"),
            }),
            artifacts: vec![],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        },
    ));

    // Should NOT have flushed any cells yet (exploring stays active)
    let cells = drain_insert_history(&mut rx);
    assert!(
        cells.is_empty(),
        "Exploring reads should not flush to history while still groupable, got {} cells",
        cells.len()
    );

    // The active cell should contain both reads
    let blob = active_blob(&chat);
    assert!(
        blob.contains("Explored") || blob.contains("Exploring"),
        "Active cell should have Explored/Exploring header, got: {blob:?}"
    );
    assert!(
        blob.contains("file1.rs") && blob.contains("file2.rs"),
        "Active cell should contain both filenames, got: {blob:?}"
    );
}

// --- Spec 12: Execute Cell Completion Buffering ---

#[test]
fn parallel_execute_snapshots_buffer_and_complete_correctly() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.on_task_started();
    drain_insert_history(&mut rx);

    // Simulate parallel ACP execute tool calls (date, uptime, df).
    // The pattern is: pending(date), update(date, desc), pending(uptime)
    // which should displace date to buffer, then update(date, completed).

    // 1) Pending date
    chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(
        nori_protocol::ToolSnapshot {
            call_id: "toolu_date".into(),
            title: "Terminal".into(),
            kind: nori_protocol::ToolKind::Execute,
            phase: nori_protocol::ToolPhase::Pending,
            locations: vec![],
            invocation: Some(nori_protocol::Invocation::Command {
                command: "date --utc".into(),
            }),
            artifacts: vec![],
            raw_input: Some(serde_json::json!({"command": "date --utc"})),
            raw_output: None,
            owner_request_id: None,
        },
    ));

    // 2) In-progress date with description text
    chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(
        nori_protocol::ToolSnapshot {
            call_id: "toolu_date".into(),
            title: "date --utc".into(),
            kind: nori_protocol::ToolKind::Execute,
            phase: nori_protocol::ToolPhase::InProgress,
            locations: vec![],
            invocation: Some(nori_protocol::Invocation::Command {
                command: "date --utc".into(),
            }),
            artifacts: vec![nori_protocol::Artifact::Text {
                text: "Print current UTC date/time".into(),
            }],
            raw_input: Some(serde_json::json!({"command": "date --utc"})),
            raw_output: None,
            owner_request_id: None,
        },
    ));

    // 3) Pending uptime — this displaces date from active_cell
    chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(
        nori_protocol::ToolSnapshot {
            call_id: "toolu_uptime".into(),
            title: "Terminal".into(),
            kind: nori_protocol::ToolKind::Execute,
            phase: nori_protocol::ToolPhase::Pending,
            locations: vec![],
            invocation: Some(nori_protocol::Invocation::Command {
                command: "uptime -p".into(),
            }),
            artifacts: vec![],
            raw_input: Some(serde_json::json!({"command": "uptime -p"})),
            raw_output: None,
            owner_request_id: None,
        },
    ));

    // 4) Completed date arrives (should find it in buffer, not discard it)
    chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(
        nori_protocol::ToolSnapshot {
            call_id: "toolu_date".into(),
            title: "date --utc".into(),
            kind: nori_protocol::ToolKind::Execute,
            phase: nori_protocol::ToolPhase::Completed,
            locations: vec![],
            invocation: Some(nori_protocol::Invocation::Command {
                command: "date --utc".into(),
            }),
            artifacts: vec![nori_protocol::Artifact::Text {
                text: "2026-03-30 05:45:34 UTC".into(),
            }],
            raw_input: Some(serde_json::json!({"command": "date --utc"})),
            raw_output: Some(serde_json::json!({
                "exit_code": 0,
                "stdout": "2026-03-30 05:45:34 UTC"
            })),
            owner_request_id: None,
        },
    ));

    // 5) Completed uptime
    chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(
        nori_protocol::ToolSnapshot {
            call_id: "toolu_uptime".into(),
            title: "uptime -p".into(),
            kind: nori_protocol::ToolKind::Execute,
            phase: nori_protocol::ToolPhase::Completed,
            locations: vec![],
            invocation: Some(nori_protocol::Invocation::Command {
                command: "uptime -p".into(),
            }),
            artifacts: vec![nori_protocol::Artifact::Text {
                text: "up 1 week, 2 days".into(),
            }],
            raw_input: Some(serde_json::json!({"command": "uptime -p"})),
            raw_output: Some(serde_json::json!({
                "exit_code": 0,
                "stdout": "up 1 week, 2 days"
            })),
            owner_request_id: None,
        },
    ));

    // Drain history and check that both commands rendered with correct output
    let cells = drain_insert_history(&mut rx);
    let combined: Vec<String> = cells.iter().map(|c| lines_to_single_string(c)).collect();
    let full = combined.join("");

    // Date command should have real stdout, not description
    assert!(
        full.contains("2026-03-30 05:45:34 UTC"),
        "Date command should show real stdout, got: {full:?}"
    );
    assert!(
        !full.contains("Print current UTC date/time"),
        "Description text should NOT appear as output, got: {full:?}"
    );
    // Uptime command should have real stdout
    assert!(
        full.contains("up 1 week, 2 days"),
        "Uptime command should show real stdout, got: {full:?}"
    );
}

#[test]
fn orphan_buffered_execute_cell_discarded_on_turn_complete() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.on_task_started();
    drain_insert_history(&mut rx);

    // 1) Pending execute cell (date)
    chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(
        nori_protocol::ToolSnapshot {
            call_id: "toolu_orphan".into(),
            title: "date --utc".into(),
            kind: nori_protocol::ToolKind::Execute,
            phase: nori_protocol::ToolPhase::InProgress,
            locations: vec![],
            invocation: Some(nori_protocol::Invocation::Command {
                command: "date --utc".into(),
            }),
            artifacts: vec![nori_protocol::Artifact::Text {
                text: "Print current UTC date/time".into(),
            }],
            raw_input: Some(serde_json::json!({"command": "date --utc"})),
            raw_output: None,
            owner_request_id: None,
        },
    ));

    // 2) Another execute displaces the first one
    chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(
        nori_protocol::ToolSnapshot {
            call_id: "toolu_second".into(),
            title: "uptime -p".into(),
            kind: nori_protocol::ToolKind::Execute,
            phase: nori_protocol::ToolPhase::Completed,
            locations: vec![],
            invocation: Some(nori_protocol::Invocation::Command {
                command: "uptime -p".into(),
            }),
            artifacts: vec![],
            raw_input: Some(serde_json::json!({"command": "uptime -p"})),
            raw_output: Some(serde_json::json!({
                "exit_code": 0,
                "stdout": "up 1 week"
            })),
            owner_request_id: None,
        },
    ));

    // 3) Turn completes without the first cell ever completing
    chat.on_task_complete(None);

    let cells = drain_insert_history(&mut rx);
    let combined: Vec<String> = cells.iter().map(|c| lines_to_single_string(c)).collect();
    let full = combined.join("");

    // The orphan cell should NOT appear in history with its description text
    assert!(
        !full.contains("Print current UTC date/time"),
        "Orphan buffered cell should be discarded, not show description as output: {full:?}"
    );
    // The completed second cell should appear
    assert!(
        full.contains("up 1 week"),
        "Completed cell should appear in history: {full:?}"
    );
}

#[test]
fn description_text_not_shown_as_execute_output() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    // Send in-progress execute with description-only content (no raw_output)
    chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(
        nori_protocol::ToolSnapshot {
            call_id: "toolu_desc".into(),
            title: "rm /tmp/test.md".into(),
            kind: nori_protocol::ToolKind::Execute,
            phase: nori_protocol::ToolPhase::InProgress,
            locations: vec![],
            invocation: Some(nori_protocol::Invocation::Command {
                command: "rm /tmp/test.md".into(),
            }),
            artifacts: vec![nori_protocol::Artifact::Text {
                text: "Delete the temporary test file".into(),
            }],
            raw_input: Some(serde_json::json!({"command": "rm /tmp/test.md"})),
            raw_output: None,
            owner_request_id: None,
        },
    ));

    let blob = active_blob(&chat);
    assert!(
        blob.contains("Running"),
        "In-progress execute should show 'Running': {blob:?}"
    );
    assert!(
        !blob.contains("Delete the temporary test file"),
        "Description text should NOT appear as output: {blob:?}"
    );
}

#[test]
fn single_read_snapshot_renders_as_explored() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(
        nori_protocol::ToolSnapshot {
            call_id: "call-single-read".into(),
            title: "Read README.md".into(),
            kind: nori_protocol::ToolKind::Read,
            phase: nori_protocol::ToolPhase::Completed,
            locations: vec![],
            invocation: Some(nori_protocol::Invocation::Read {
                path: PathBuf::from("README.md"),
            }),
            artifacts: vec![],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        },
    ));

    let blob = active_blob(&chat);
    // Should show "Explored", not "Ran Read File" or "Tool [completed]"
    assert!(
        blob.contains("Explored"),
        "Single read should render as 'Explored', got: {blob:?}"
    );
    assert!(
        !blob.contains("Ran"),
        "Single read should NOT show 'Ran', got: {blob:?}"
    );
    assert!(
        !blob.contains("Tool ["),
        "Single read should NOT use generic format, got: {blob:?}"
    );
    assert!(
        blob.contains("README.md"),
        "Should show the filename, got: {blob:?}"
    );
}

#[test]
fn list_files_title_not_duplicated() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    // Send a ListFiles snapshot with title "List src" and kind Other("List")
    // This exercises the generic fallback path in render_exploring_lines
    chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(
        nori_protocol::ToolSnapshot {
            call_id: "call-list-dup".into(),
            title: "List /home/user/project/src".into(),
            kind: nori_protocol::ToolKind::Other("List".into()),
            phase: nori_protocol::ToolPhase::Completed,
            locations: vec![],
            invocation: Some(nori_protocol::Invocation::ListFiles {
                path: Some(PathBuf::from("/home/user/project/src")),
            }),
            artifacts: vec![],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        },
    ));

    let blob = active_blob(&chat);
    // Should NOT show "List List"
    assert!(
        !blob.contains("List List"),
        "Should not duplicate 'List' label, got: {blob:?}"
    );
    // Should still show the path
    assert!(
        blob.contains("src"),
        "Should still show the path, got: {blob:?}"
    );
}

// --- ACP Edit Approval Bridge Removal ---

/// ACP edit approval requests should route through AcpTool (not ApplyPatch).
/// This gives users the "always approve" option and uses native protocol rendering.
#[test]
fn acp_edit_approval_routes_through_acp_tool() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    // Send an ACP approval request for an Edit tool with file changes
    chat.handle_client_event(nori_protocol::ClientEvent::ApprovalRequest(
        nori_protocol::ApprovalRequest {
            call_id: "call-edit-approval".into(),
            title: "Edit src/main.rs".into(),
            kind: nori_protocol::ToolKind::Edit,
            options: vec![],
            subject: nori_protocol::ApprovalSubject::ToolSnapshot(nori_protocol::ToolSnapshot {
                call_id: "call-edit-approval".into(),
                title: "Edit src/main.rs".into(),
                kind: nori_protocol::ToolKind::Edit,
                phase: nori_protocol::ToolPhase::PendingApproval,
                locations: vec![nori_protocol::ToolLocation {
                    path: PathBuf::from("src/main.rs"),
                    line: None,
                }],
                invocation: Some(nori_protocol::Invocation::FileChanges {
                    changes: vec![nori_protocol::FileChange {
                        path: PathBuf::from("src/main.rs"),
                        old_text: Some("fn main() {}\n".into()),
                        new_text: "fn main() {\n    println!(\"hello\");\n}\n".into(),
                    }],
                }),
                artifacts: vec![nori_protocol::Artifact::Diff(nori_protocol::FileChange {
                    path: PathBuf::from("src/main.rs"),
                    old_text: Some("fn main() {}\n".into()),
                    new_text: "fn main() {\n    println!(\"hello\");\n}\n".into(),
                })],
                raw_input: None,
                raw_output: None,
                owner_request_id: None,
            }),
        },
    ));

    // Render the bottom pane and check the approval text
    let width: u16 = 80;
    let height: u16 = 30;
    let mut buf = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, width, height));
    chat.bottom_pane
        .render(ratatui::layout::Rect::new(0, 0, width, height), &mut buf);

    let rendered: Vec<String> = (0..height)
        .map(|row| {
            (0..width)
                .map(|col| buf[(col, row)].symbol().to_string())
                .collect()
        })
        .collect();

    // AcpTool renders "Would you like to allow edit: ..."
    // ApplyPatch renders "Would you like to make the following edits?"
    assert!(
        rendered
            .iter()
            .any(|line| line.contains("Would you like to allow")),
        "ACP edit approval should route through AcpTool ('Would you like to allow'), got: {rendered:?}"
    );
    assert!(
        !rendered
            .iter()
            .any(|line| line.contains("Would you like to make the following edits")),
        "ACP edit approval should NOT use ApplyPatch text, got: {rendered:?}"
    );

    // Approve with 'y' and verify it sends ExecApproval (AcpTool path), not PatchApproval
    chat.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('y'),
        crossterm::event::KeyModifiers::NONE,
    ));

    // The approval overlay sends Op via AppEvent::CodexOp through app_event_tx
    let mut found_exec_approval = false;
    let mut found_patch_approval = false;
    while let Ok(event) = rx.try_recv() {
        if let AppEvent::CodexOp(op) = event {
            match op {
                Op::ExecApproval { .. } => found_exec_approval = true,
                Op::PatchApproval { .. } => found_patch_approval = true,
                _ => {}
            }
        }
    }
    assert!(
        found_exec_approval,
        "ACP edit approval should emit Op::ExecApproval (AcpTool path)"
    );
    assert!(
        !found_patch_approval,
        "ACP edit approval should NOT emit Op::PatchApproval"
    );
}

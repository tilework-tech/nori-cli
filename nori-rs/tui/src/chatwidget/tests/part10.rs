use super::*;

#[test]
fn candidate_login_target_survives_failure_and_clears_after_success() {
    let (mut chat, _rx, _unused_rx) = make_chatwidget_manual();
    chat.config.active_agent = "claude-code".to_string();
    chat.set_login_agent_override(Some("codex".to_string()));
    chat.login_handler = Some(crate::login_handler::LoginHandler::new());

    chat.handle_login_complete(false);
    assert_eq!(
        chat.login_target_agent(),
        "codex",
        "a failed login should keep routing a retry to the candidate"
    );

    chat.login_handler = Some(crate::login_handler::LoginHandler::new());
    chat.handle_login_complete(true);
    assert_eq!(
        chat.login_target_agent(),
        "claude-code",
        "successful authentication should restore bare /login to the active agent"
    );
}

#[test]
fn deferred_startup_input_preserves_prompt_and_image_only_semantics_for_resume() {
    let (mut chat, _rx, _unused_rx) = make_cloud_chatwidget_manual();
    chat.first_prompt_text = Some("Continue onboarding".to_string());
    chat.initial_user_message = Some(UserMessage {
        text: "Continue onboarding".to_string(),
        image_paths: vec![PathBuf::from("diagram.png")],
    });

    let (prompt, images) = chat.take_initial_input();

    assert_eq!(prompt.as_deref(), Some("Continue onboarding"));
    assert_eq!(images, vec![PathBuf::from("diagram.png")]);
    assert!(chat.initial_user_message.is_none());

    chat.initial_user_message = Some(UserMessage {
        text: String::new(),
        image_paths: vec![PathBuf::from("screenshot.png")],
    });
    let (prompt, images) = chat.take_initial_input();

    assert_eq!(prompt, None);
    assert_eq!(images, vec![PathBuf::from("screenshot.png")]);

    chat.first_prompt_text = Some("Already submitted".to_string());
    let (prompt, images) = chat.take_initial_input();

    assert_eq!(prompt, None);
    assert!(images.is_empty());
    assert_eq!(chat.first_prompt_text.as_deref(), Some("Already submitted"));
}

#[test]
fn switch_candidate_can_clone_deferred_input_without_consuming_the_rollback_copy() {
    let (mut chat, _rx, _unused_rx) = make_cloud_chatwidget_manual();
    chat.first_prompt_text = Some("Continue onboarding".to_string());
    chat.initial_user_message = Some(UserMessage {
        text: "Continue onboarding".to_string(),
        image_paths: vec![PathBuf::from("diagram.png")],
    });

    let cloned = chat.clone_initial_input();
    let retained = chat.take_initial_input();

    assert_eq!(cloned, retained);
    assert_eq!(cloned.0.as_deref(), Some("Continue onboarding"));
    assert_eq!(cloned.1, vec![PathBuf::from("diagram.png")]);
}

#[test]
fn switch_candidate_session_start_retains_initial_input_until_release() {
    let (mut chat, _rx, _op_rx) = make_cloud_candidate_chatwidget_manual();
    chat.initial_user_message = Some(UserMessage {
        text: "Continue remotely".to_string(),
        image_paths: vec![PathBuf::from("diagram.png")],
    });
    let generation = chat.session_generation;

    chat.handle_session_event(generation, session_started_event("candidate-session"));

    let retained = chat
        .initial_user_message
        .as_ref()
        .expect("candidate input must remain deferred until the remote host attaches");
    assert_eq!(retained.text, "Continue remotely");
    assert_eq!(retained.image_paths, vec![PathBuf::from("diagram.png")]);
    assert!(chat.defer_initial_user_message_until_commit);

    chat.submit_candidate_initial_user_message();

    assert!(
        chat.initial_user_message.is_none(),
        "committing the candidate should release its deferred input"
    );
    assert!(!chat.defer_initial_user_message_until_commit);
}

/// Deliver a prompt completion through the real client-event entry point.
fn deliver_completion(chat: &mut ChatWidget, failure: Option<crate::presentation::TurnFailure>) {
    chat.handle_client_event(crate::presentation::ClientEvent::PromptCompleted(
        crate::presentation::PromptCompleted {
            stop_reason: nori_protocol::acp::v1::StopReason::Cancelled,
            last_agent_message: None,
            failure,
        },
    ));
}

fn history_text(rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>) -> String {
    drain_insert_history(rx)
        .iter()
        .map(|cell| lines_to_single_string(cell))
        .collect()
}

/// Scan the app-event stream for the next loop re-fire, if any.
fn next_loop_iteration(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) -> Option<(i32, i32)> {
    while let Ok(ev) = rx.try_recv() {
        if let AppEvent::LoopIteration {
            remaining, total, ..
        } = ev
        {
            return Some((remaining, total));
        }
    }
    None
}

fn drain_loop_iterations_and_history(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) -> (Vec<(i32, i32)>, String) {
    let mut loop_iterations = Vec::new();
    let mut history = String::new();
    while let Ok(event) = rx.try_recv() {
        match event {
            AppEvent::LoopIteration {
                remaining, total, ..
            } => loop_iterations.push((remaining, total)),
            AppEvent::InsertHistoryCell(cell) => {
                for line in cell.display_lines(80) {
                    for span in line.spans {
                        history.push_str(&span.content);
                    }
                    history.push('\n');
                }
            }
            _ => {}
        }
    }
    (loop_iterations, history)
}

fn deliver_prompt_failure(
    chat: &mut ChatWidget,
    request_id: nori_protocol::acp::v1::RequestId,
    kind: nori_protocol::RequestFailureKind,
    raw_error_first: bool,
) {
    let generation = chat.session_generation;
    chat.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::SessionPhaseChanged(
            nori_protocol::SessionPhase::Prompting {
                request_id: request_id.clone(),
            },
        )),
    );
    let raw_error = nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Response {
        request_id: request_id.clone(),
        response: Err(nori_protocol::acp::v1::Error::new(
            -32001,
            "raw ACP prompt error",
        )),
    });
    let classified_failure = nori_protocol::SessionEvent::Nori(
        nori_protocol::NoriEvent::RequestFailed(nori_protocol::RequestFailure {
            request_id: Some(request_id),
            message: "classified prompt failure".to_string(),
            kind,
        }),
    );
    let events = if raw_error_first {
        [raw_error, classified_failure]
    } else {
        [classified_failure, raw_error]
    };
    for event in events {
        chat.handle_session_event(generation, event);
    }
}

fn replay_message_update(
    update: nori_protocol::acp::v1::SessionUpdate,
) -> nori_protocol::SessionEvent {
    nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Notification(
        nori_protocol::acp::v1::AgentNotification::SessionNotification(
            nori_protocol::acp::v1::SessionNotification::new("session", update),
        ),
    ))
}

fn initialize_agent_event(name: &str, title: &str, version: &str) -> nori_protocol::SessionEvent {
    nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Response {
        request_id: nori_protocol::acp::v1::RequestId::Str("initialize".to_string()),
        response: Ok(nori_protocol::acp::v1::AgentResponse::InitializeResponse(
            nori_protocol::acp::v1::InitializeResponse::new(
                nori_protocol::acp::ProtocolVersion::LATEST,
            )
            .agent_info(nori_protocol::acp::v1::Implementation::new(name, version).title(title)),
        )),
    })
}

fn initialize_agent_with_load_and_resume() -> nori_protocol::SessionEvent {
    let capabilities = nori_protocol::acp::v1::AgentCapabilities::new()
        .load_session(true)
        .session_capabilities(
            nori_protocol::acp::v1::SessionCapabilities::new()
                .resume(nori_protocol::acp::v1::SessionResumeCapabilities::new()),
        );
    nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Response {
        request_id: nori_protocol::acp::v1::RequestId::Str("initialize".to_string()),
        response: Ok(nori_protocol::acp::v1::AgentResponse::InitializeResponse(
            nori_protocol::acp::v1::InitializeResponse::new(
                nori_protocol::acp::ProtocolVersion::LATEST,
            )
            .agent_capabilities(capabilities),
        )),
    })
}

fn initialize_agent_with_session_close() -> nori_protocol::SessionEvent {
    let capabilities = nori_protocol::acp::v1::AgentCapabilities::new().session_capabilities(
        nori_protocol::acp::v1::SessionCapabilities::new()
            .close(nori_protocol::acp::v1::SessionCloseCapabilities::new()),
    );
    nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Response {
        request_id: nori_protocol::acp::v1::RequestId::Str("initialize".to_string()),
        response: Ok(nori_protocol::acp::v1::AgentResponse::InitializeResponse(
            nori_protocol::acp::v1::InitializeResponse::new(
                nori_protocol::acp::ProtocolVersion::LATEST,
            )
            .agent_capabilities(capabilities),
        )),
    })
}

fn session_started_event(acp_session_id: &str) -> nori_protocol::SessionEvent {
    nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::SessionStarted(
        nori_protocol::SessionStarted {
            transcript_id: None,
            acp_session_id: nori_protocol::acp::v1::SessionId::new(acp_session_id),
            cwd: std::path::PathBuf::from("/workspace"),
            transcript_path: None,
            history_log_id: 0,
            history_entry_count: 0,
        },
    ))
}

#[test]
fn cloud_identity_survives_load_session_capability() {
    let (mut chat, _rx, _op_rx) = make_cloud_chatwidget_manual();
    let generation = chat.session_generation;

    chat.handle_session_event(generation, initialize_agent_with_load_and_resume());
    chat.handle_session_event(generation, session_started_event("cloud-session"));

    let identity = chat
        .cloud_session_identity()
        .expect("cloud launch should retain the ACP session identity");
    assert_eq!(identity.id, "cloud-session");
}

#[test]
fn local_mode_does_not_become_cloud_from_capabilities() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    let generation = chat.session_generation;

    chat.handle_session_event(generation, initialize_agent_with_load_and_resume());
    chat.handle_session_event(generation, session_started_event("local-session"));

    assert!(chat.cloud_session_identity().is_none());
    assert!(
        chat.ensure_builtin_command_enabled(crate::slash_command::SlashCommand::Browse),
        "agent capabilities must not disable local commands"
    );
}

#[test]
fn cloud_mode_disables_local_commands_when_load_session_is_supported() {
    let (mut chat, mut rx, _op_rx) = make_cloud_chatwidget_manual();
    let generation = chat.session_generation;

    chat.handle_session_event(generation, initialize_agent_with_load_and_resume());

    assert!(
        !chat.ensure_builtin_command_enabled(crate::slash_command::SlashCommand::Browse),
        "cloud launch origin must disable local-only commands"
    );
    let rendered = history_text(&mut rx);
    assert!(
        rendered
            .contains("/browse runs on the local machine and is unavailable in cloud sessions."),
        "the command error should explain the cloud boundary"
    );
    insta::assert_snapshot!("cloud_local_command_error_with_load_session", rendered);
}

#[test]
fn local_mode_rejects_cloud_close_even_when_the_agent_supports_it() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let generation = chat.session_generation;

    chat.handle_session_event(generation, initialize_agent_with_session_close());

    assert!(
        !chat.ensure_builtin_command_enabled(crate::slash_command::SlashCommand::Close),
        "session/close support must not turn a local launch into a cloud session"
    );
    assert!(
        history_text(&mut rx).contains("/close is available only in cloud sessions."),
        "the command error should explain the explicit cloud boundary"
    );
}

#[test]
fn cloud_quit_remains_a_detach_when_load_session_is_supported() {
    let (mut chat, mut rx, _op_rx) = make_cloud_chatwidget_manual();
    let generation = chat.session_generation;

    chat.handle_session_event(generation, initialize_agent_with_load_and_resume());
    chat.handle_session_event(generation, session_started_event("cloud-session"));
    let _ = history_text(&mut rx);
    chat.begin_exit();

    let rendered = history_text(&mut rx);
    assert!(
        rendered.contains("This session keeps running in the cloud."),
        "quitting cloud mode should explain that it detaches"
    );
    insta::assert_snapshot!("cloud_quit_detach_feedback_with_load_session", rendered);
}

#[test]
fn cloud_picker_exit_does_not_claim_an_unattached_session_keeps_running() {
    let (mut chat, mut rx, _op_rx) = make_cloud_chatwidget_manual();

    chat.begin_exit();

    assert!(
        !history_text(&mut rx).contains("This session keeps running in the cloud."),
        "cloud launch origin alone does not mean a session was attached"
    );
}

fn session_info_update(
    update: nori_protocol::acp::v1::SessionInfoUpdate,
) -> nori_protocol::SessionEvent {
    replay_message_update(nori_protocol::acp::v1::SessionUpdate::SessionInfoUpdate(
        update,
    ))
}

#[test]
fn session_info_updates_render_known_codex_fields() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let generation = chat.session_generation;
    let meta = serde_json::json!({
        "codex": {
            "threadStatus": {
                "type": "active",
                "activeFlags": ["waitingOnApproval"]
            },
            "goal": {
                "objective": "Ship metadata visibility",
                "status": "active",
                "tokenBudget": 20000,
                "timeUsedSeconds": 42,
                "createdAt": 1784977200,
                "controlMethod": "_codex/session/goal_control"
            },
            "error": {
                "message": "temporary overload",
                "turnId": "turn-7",
                "willRetry": true
            },
            "archived": false,
            "closed": false
        }
    })
    .as_object()
    .expect("metadata object")
    .clone();

    chat.handle_session_event(
        generation,
        initialize_agent_event("codex-acp", "Codex ACP", "1.1.4"),
    );
    chat.handle_session_event(
        generation,
        session_info_update(
            nori_protocol::acp::v1::SessionInfoUpdate::new()
                .title("Metadata work")
                .updated_at("2026-07-25T12:00:00Z")
                .meta(meta),
        ),
    );

    let rendered = history_text(&mut rx);
    for expected in [
        "Codex ACP 1.1.4 session updated",
        "title=Metadata work",
        "updated_at=2026-07-25T12:00:00Z",
        "status=active",
        "waiting=approval",
        "goal.objective=Ship metadata visibility",
        "goal.status=active",
        "goal.token_budget=20,000",
        "goal.time_used=42s",
        "error.message=temporary overload",
        "error.turn_id=turn-7",
        "error.will_retry=true",
        "archived=false",
        "closed=false",
    ] {
        assert!(
            rendered.contains(expected),
            "expected {expected:?} in session-info history:\n{rendered}"
        );
    }
    insta::assert_snapshot!("session_info_update_rich_history", rendered);
}

#[test]
fn stable_builds_drop_the_metadata_cell_but_keep_the_session_title() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let generation = chat.session_generation;
    chat.session_info_detail = crate::nori::session_info::SessionInfoDetail::ErrorsOnly;
    let meta = serde_json::json!({"codex": {"threadStatus": {"type": "idle"}}})
        .as_object()
        .expect("metadata object")
        .clone();

    chat.handle_session_event(
        generation,
        initialize_agent_event("codex-acp", "Codex ACP", "1.1.4"),
    );
    chat.handle_session_event(
        generation,
        session_info_update(
            nori_protocol::acp::v1::SessionInfoUpdate::new()
                .title("Metadata work")
                .updated_at("2026-07-25T12:00:00Z")
                .meta(meta),
        ),
    );

    assert_eq!(history_text(&mut rx), String::new());
    assert_eq!(
        chat.bottom_pane.status_card_info().session_title,
        Some("Metadata work".to_string())
    );
}

#[test]
fn stable_builds_still_report_agent_errors() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let generation = chat.session_generation;
    chat.session_info_detail = crate::nori::session_info::SessionInfoDetail::ErrorsOnly;
    let meta = serde_json::json!({
        "codex": {
            "threadStatus": {"type": "systemError"},
            "error": {"message": "temporary overload", "willRetry": true}
        }
    })
    .as_object()
    .expect("metadata object")
    .clone();

    chat.handle_session_event(
        generation,
        initialize_agent_event("codex-acp", "Codex ACP", "1.1.4"),
    );
    chat.handle_session_event(
        generation,
        session_info_update(nori_protocol::acp::v1::SessionInfoUpdate::new().meta(meta)),
    );

    insta::assert_snapshot!(history_text(&mut rx), @r"
    • Codex ACP 1.1.4 session updated:
      error.message=temporary overload, error.will_retry=true
    ");
}

#[test]
fn session_titles_are_bounded_and_stripped_of_control_characters() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    let generation = chat.session_generation;

    chat.handle_session_event(
        generation,
        session_info_update(
            nori_protocol::acp::v1::SessionInfoUpdate::new()
                .title("Fix the login flake\nin the auth integration test suite before release"),
        ),
    );

    assert_eq!(
        chat.bottom_pane.status_card_info().session_title,
        Some("Fix the login flake in the auth integration test…".to_string())
    );
}

#[test]
fn nori_connection_status_updates_render_clear_messages() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let generation = chat.session_generation;

    for status in ["reconnecting", "connected"] {
        let meta = serde_json::json!({
            "nori": {
                "connection": {
                    "status": status
                }
            }
        })
        .as_object()
        .expect("metadata object")
        .clone();
        chat.handle_session_event(
            generation,
            session_info_update(nori_protocol::acp::v1::SessionInfoUpdate::new().meta(meta)),
        );
    }

    let rendered = history_text(&mut rx);
    insta::assert_snapshot!(rendered, @r"
• Cloud connection lost. Reconnecting…

• Cloud connection restored.
");
}

#[test]
fn nori_connection_status_with_other_metadata_uses_standard_rendering() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let generation = chat.session_generation;
    let meta = serde_json::json!({
        "nori": {
            "connection": {
                "status": "reconnecting"
            }
        },
        "vendor": {
            "field": true
        }
    })
    .as_object()
    .expect("metadata object")
    .clone();

    chat.handle_session_event(
        generation,
        session_info_update(nori_protocol::acp::v1::SessionInfoUpdate::new().meta(meta)),
    );

    let rendered = history_text(&mut rx);
    assert!(
        rendered.contains("nori.connection.status=<string>"),
        "{rendered}"
    );
    assert!(rendered.contains("vendor.field=<boolean>"), "{rendered}");
    assert!(
        !rendered.contains("Cloud connection lost. Reconnecting…"),
        "{rendered}"
    );
}

#[test]
fn unknown_session_info_fields_render_types_without_values_in_stable_order() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let generation = chat.session_generation;
    let meta = serde_json::json!({
        "codex": {
            "futureField": "must-not-leak"
        },
        "vendor": {
            "empty": {},
            "flag": true,
            "items": ["also-private"],
            "nothing": null,
            "payload": {
                "count": 7
            },
            "secret": "also-must-not-leak"
        }
    })
    .as_object()
    .expect("metadata object")
    .clone();

    chat.handle_session_event(
        generation,
        initialize_agent_event("codex-acp", "Codex ACP", "1.1.4"),
    );
    chat.handle_session_event(
        generation,
        session_info_update(nori_protocol::acp::v1::SessionInfoUpdate::new().meta(meta)),
    );

    let rendered = history_text(&mut rx);
    let expected = [
        "codex.futureField=<string>",
        "vendor.empty=<object>",
        "vendor.flag=<boolean>",
        "vendor.items=<array>",
        "vendor.nothing=<null>",
        "vendor.payload.count=<number>",
        "vendor.secret=<string>",
    ];
    let positions = expected
        .iter()
        .map(|field| {
            rendered
                .find(field)
                .unwrap_or_else(|| panic!("expected {field:?} in:\n{rendered}"))
        })
        .collect::<Vec<_>>();
    assert!(
        positions.windows(2).all(|window| window[0] < window[1]),
        "unknown paths should be sorted:\n{rendered}"
    );
    for private_value in ["must-not-leak", "also-private", "also-must-not-leak"] {
        assert!(!rendered.contains(private_value), "{rendered}");
    }
}

#[test]
fn codex_metadata_from_a_custom_agent_uses_the_unknown_fallback() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let generation = chat.session_generation;
    let meta = serde_json::json!({
        "codex": {
            "threadStatus": {
                "type": "active",
                "activeFlags": ["waitingOnApproval"]
            }
        }
    })
    .as_object()
    .expect("metadata object")
    .clone();

    chat.handle_session_event(
        generation,
        initialize_agent_event("custom-agent", "Custom Agent", "2.0.0"),
    );
    chat.handle_session_event(
        generation,
        session_info_update(nori_protocol::acp::v1::SessionInfoUpdate::new().meta(meta)),
    );

    let rendered = history_text(&mut rx);
    assert!(
        rendered.contains("codex.threadStatus.activeFlags=<array>"),
        "{rendered}"
    );
    assert!(
        rendered.contains("codex.threadStatus.type=<string>"),
        "{rendered}"
    );
    assert!(!rendered.contains("status=active"), "{rendered}");
    assert!(!rendered.contains("waiting=approval"), "{rendered}");
}

#[test]
fn malformed_known_codex_fields_use_the_unknown_fallback() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let generation = chat.session_generation;
    let excessive_flags = vec!["waitingOnApproval"; 100];
    let meta = serde_json::json!({
        "codex": {
            "threadStatus": {
                "type": {"unexpected": "must-not-leak"},
                "activeFlags": excessive_flags
            },
            "archived": "must-not-leak"
        }
    })
    .as_object()
    .expect("metadata object")
    .clone();

    chat.handle_session_event(
        generation,
        initialize_agent_event("codex-acp", "Codex ACP", "1.1.4"),
    );
    chat.handle_session_event(
        generation,
        session_info_update(nori_protocol::acp::v1::SessionInfoUpdate::new().meta(meta)),
    );

    let rendered = history_text(&mut rx);
    for expected in [
        "codex.archived=<string>",
        "codex.threadStatus.activeFlags=<array>",
        "codex.threadStatus.type.unexpected=<string>",
    ] {
        assert!(rendered.contains(expected), "{rendered}");
    }
    assert!(!rendered.contains("must-not-leak"), "{rendered}");
}

#[test]
fn session_info_headers_sanitize_and_bound_agent_identity() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let generation = chat.session_generation;
    let oversized_version = "v".repeat(500);
    let oversized_name = "n".repeat(100_000);
    let meta = serde_json::json!({"vendor": {"field": true}})
        .as_object()
        .expect("metadata object")
        .clone();

    chat.handle_session_event(
        generation,
        initialize_agent_event(
            &oversized_name,
            "Unsafe\nAgent\u{1b}[31m",
            &oversized_version,
        ),
    );
    chat.handle_session_event(
        generation,
        session_info_update(
            nori_protocol::acp::v1::SessionInfoUpdate::new()
                .title("Safe field")
                .meta(meta),
        ),
    );

    let rendered = history_text(&mut rx);
    assert!(!rendered.contains('\u{1b}'), "{rendered:?}");
    assert_eq!(
        rendered.lines().count(),
        2,
        "agent identity must not inject history lines: {rendered:?}"
    );
    assert!(
        rendered.lines().all(|line| line.chars().count() <= 240),
        "agent identity must be bounded: {rendered:?}"
    );
}

#[test]
fn deeply_nested_unknown_metadata_stops_at_a_bounded_path() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let generation = chat.session_generation;
    let mut nested = serde_json::json!("private-value");
    for depth in (0..40).rev() {
        nested = serde_json::json!({format!("level{depth}"): nested});
    }
    let meta = serde_json::json!({"vendor": nested})
        .as_object()
        .expect("metadata object")
        .clone();

    chat.handle_session_event(
        generation,
        initialize_agent_event("custom-agent", "Custom Agent", "2.0.0"),
    );
    chat.handle_session_event(
        generation,
        session_info_update(nori_protocol::acp::v1::SessionInfoUpdate::new().meta(meta)),
    );

    let rendered = history_text(&mut rx);
    assert!(rendered.contains("=<object>"), "{rendered}");
    assert!(!rendered.contains("level39=<string>"), "{rendered}");
    assert!(!rendered.contains("private-value"), "{rendered}");
}

#[test]
fn replayed_session_info_updates_identify_the_replay_source() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let generation = chat.session_generation;
    chat.handle_session_event(
        generation,
        initialize_agent_event("claude-agent-acp", "Claude Agent ACP", "0.62.0"),
    );
    chat.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::ReplayStarted(
            nori_protocol::ReplayStarted {
                source: nori_protocol::ReplaySource::Agent,
            },
        )),
    );
    chat.handle_session_event(
        generation,
        session_info_update(
            nori_protocol::acp::v1::SessionInfoUpdate::new().title("Recovered title"),
        ),
    );
    chat.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::ReplayFinished),
    );

    let replayed = history_text(&mut rx);
    assert!(
        replayed.contains("Claude Agent ACP 0.62.0 session updated (agent replay)"),
        "{replayed}"
    );
    assert!(replayed.contains("title=Recovered title"), "{replayed}");

    chat.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::ReplayStarted(
            nori_protocol::ReplayStarted {
                source: nori_protocol::ReplaySource::Transcript,
            },
        )),
    );
    chat.handle_session_event(
        generation,
        session_info_update(
            nori_protocol::acp::v1::SessionInfoUpdate::new().title("Transcript title"),
        ),
    );
    chat.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::ReplayFinished),
    );
    let transcript_replay = history_text(&mut rx);
    assert!(
        transcript_replay.contains("Claude Agent ACP 0.62.0 session updated (transcript replay)"),
        "{transcript_replay}"
    );

    chat.handle_session_event(
        generation,
        session_info_update(nori_protocol::acp::v1::SessionInfoUpdate::new().title("Live title")),
    );
    let live = history_text(&mut rx);
    assert!(
        live.contains("Claude Agent ACP 0.62.0 session updated"),
        "{live}"
    );
    assert!(!live.contains("(agent replay)"), "{live}");
    assert!(!live.contains("(transcript replay)"), "{live}");
}

fn replay_text_chunk(
    stream: crate::presentation::MessageStream,
    message_id: &str,
    text: &str,
) -> nori_protocol::SessionEvent {
    let chunk = nori_protocol::acp::v1::ContentChunk::new(
        nori_protocol::acp::v1::ContentBlock::Text(nori_protocol::acp::v1::TextContent::new(text)),
    )
    .message_id(message_id);
    let update = match stream {
        crate::presentation::MessageStream::User => {
            nori_protocol::acp::v1::SessionUpdate::UserMessageChunk(chunk)
        }
        crate::presentation::MessageStream::Answer => {
            nori_protocol::acp::v1::SessionUpdate::AgentMessageChunk(chunk)
        }
        crate::presentation::MessageStream::Reasoning => {
            nori_protocol::acp::v1::SessionUpdate::AgentThoughtChunk(chunk)
        }
    };
    replay_message_update(update)
}

fn nori_status_update(status: &str) -> nori_protocol::SessionEvent {
    let mut meta = serde_json::Map::new();
    meta.insert("nori".to_string(), serde_json::json!({ "status": status }));
    replay_message_update(nori_protocol::acp::v1::SessionUpdate::SessionInfoUpdate(
        nori_protocol::acp::v1::SessionInfoUpdate::new().meta(meta),
    ))
}

#[test]
fn proactive_turn_status_bounds_unowned_output_without_echoing_owned_prompts() {
    let (mut observer, mut observer_rx, _op_rx) = make_chatwidget_manual();
    let observer_generation = observer.session_generation;
    observer.handle_session_event(observer_generation, nori_status_update("working"));
    observer.handle_session_event(
        observer_generation,
        replay_text_chunk(
            crate::presentation::MessageStream::User,
            "proactive-user",
            "what's the status here so far?",
        ),
    );
    observer.handle_session_event(
        observer_generation,
        replay_text_chunk(
            crate::presentation::MessageStream::Answer,
            "proactive-answer",
            "No implementation work has started.",
        ),
    );
    observer.handle_session_event(observer_generation, nori_status_update("idle"));

    let observer_history = drain_insert_history(&mut observer_rx)
        .into_iter()
        .map(|lines| lines_to_single_string(&lines))
        .collect::<String>();
    insta::assert_snapshot!(observer_history, @r"
› what's the status here so far?


─ Worked for 0s ────────────────────────────────────────────────────────────────

• No implementation work has started.
");

    let (mut initiator, mut initiator_rx, _op_rx) = make_chatwidget_manual();
    let initiator_generation = initiator.session_generation;
    initiator.handle_session_event(
        initiator_generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::SessionPhaseChanged(
            nori_protocol::SessionPhase::Prompting {
                request_id: nori_protocol::acp::v1::RequestId::Str("local-prompt".to_string()),
            },
        )),
    );
    initiator.handle_session_event(
        initiator_generation,
        replay_text_chunk(
            crate::presentation::MessageStream::User,
            "local-user",
            "do not echo this prompt",
        ),
    );

    assert!(drain_insert_history(&mut initiator_rx).is_empty());
}

#[test]
fn proactive_turn_does_not_enable_owned_request_controls() {
    let (mut cancel_chat, mut cancel_rx, _op_rx) = make_chatwidget_manual();
    let cancel_generation = cancel_chat.session_generation;

    cancel_chat.handle_session_event(cancel_generation, nori_status_update("working"));
    cancel_chat.on_ctrl_c();

    let cancel_actions = std::iter::from_fn(|| cancel_rx.try_recv().ok())
        .filter_map(|event| match event {
            AppEvent::HarnessAction(action) => Some(action),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        !cancel_actions
            .iter()
            .any(|action| matches!(action, crate::app_event::HarnessAction::Cancel)),
        "{cancel_actions:#?}"
    );

    let (mut command_chat, mut command_rx, _op_rx) = make_chatwidget_manual();
    let command_generation = command_chat.session_generation;
    command_chat.handle_session_event(command_generation, nori_status_update("working"));
    command_chat.dispatch_command(SlashCommand::New);

    assert!(
        std::iter::from_fn(|| command_rx.try_recv().ok())
            .any(|event| matches!(event, AppEvent::NewSession))
    );
}

#[test]
fn owned_prompt_start_separates_statusless_proactive_output() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let generation = chat.session_generation;
    let request_id = nori_protocol::acp::v1::RequestId::Str("local-prompt".to_string());

    chat.handle_session_event(
        generation,
        replay_text_chunk(
            crate::presentation::MessageStream::Answer,
            "proactive-answer",
            "Agent-initiated update.",
        ),
    );
    chat.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::SessionPhaseChanged(
            nori_protocol::SessionPhase::Prompting {
                request_id: request_id.clone(),
            },
        )),
    );
    chat.handle_session_event(
        generation,
        replay_text_chunk(
            crate::presentation::MessageStream::Answer,
            "owned-answer",
            "Owned response.",
        ),
    );
    chat.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Response {
            request_id,
            response: Ok(nori_protocol::acp::v1::AgentResponse::PromptResponse(
                nori_protocol::acp::v1::PromptResponse::new(
                    nori_protocol::acp::v1::StopReason::EndTurn,
                ),
            )),
        }),
    );

    let answers = drain_insert_history(&mut rx)
        .into_iter()
        .map(|lines| lines_to_single_string(&lines))
        .filter(|cell| cell.contains("Agent-initiated update.") || cell.contains("Owned response."))
        .collect::<Vec<_>>();
    assert_eq!(answers.len(), 2, "{answers:#?}");
    assert!(answers[0].contains("Agent-initiated update."));
    assert!(answers[1].contains("Owned response."));
}

#[test]
fn replayed_turns_render_as_separate_ordered_history() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let generation = chat.session_generation;

    chat.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::ReplayStarted(
            nori_protocol::ReplayStarted {
                source: nori_protocol::ReplaySource::Agent,
            },
        )),
    );
    for event in [
        replay_text_chunk(
            crate::presentation::MessageStream::User,
            "user-1",
            "First question",
        ),
        replay_text_chunk(
            crate::presentation::MessageStream::Answer,
            "answer-1",
            "First ",
        ),
        replay_text_chunk(
            crate::presentation::MessageStream::Answer,
            "answer-1",
            "answer",
        ),
        replay_text_chunk(
            crate::presentation::MessageStream::User,
            "user-2",
            "Second question",
        ),
        replay_text_chunk(
            crate::presentation::MessageStream::Answer,
            "answer-2",
            "Second answer",
        ),
    ] {
        chat.handle_session_event(generation, event);
    }
    chat.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::ReplayFinished),
    );

    let expected = [
        "First question",
        "First answer",
        "Second question",
        "Second answer",
    ];
    let rendered = drain_insert_history(&mut rx)
        .into_iter()
        .map(|lines| lines_to_single_string(&lines))
        .collect::<Vec<_>>();
    let message_cells = rendered
        .iter()
        .filter(|cell| expected.iter().any(|message| cell.contains(message)))
        .collect::<Vec<_>>();

    assert_eq!(message_cells.len(), expected.len(), "{rendered:#?}");
    for (index, expected_message) in expected.iter().enumerate() {
        assert!(
            message_cells[index].contains(expected_message),
            "{rendered:#?}"
        );
        for other_message in expected
            .iter()
            .filter(|message| message != &expected_message)
        {
            assert!(
                !message_cells[index].contains(other_message),
                "{rendered:#?}"
            );
        }
        assert_eq!(
            rendered
                .iter()
                .map(|cell| cell.matches(expected_message).count())
                .sum::<usize>(),
            1
        );
    }
    insta::assert_snapshot!(message_cells.into_iter().cloned().collect::<String>(), @r"
› First question


• First answer


› Second question


• Second answer
");
}

#[test]
fn stale_session_events_do_not_reach_the_current_widget() {
    let (old_chat, _old_rx, _old_op_rx) = make_chatwidget_manual();
    let stale_generation = old_chat.session_generation;
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let current_generation = chat.session_generation;

    chat.handle_session_event(
        stale_generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::Notice(
            nori_protocol::Notice {
                message: "stale session output".to_string(),
            },
        )),
    );
    chat.handle_session_event(
        current_generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::Notice(
            nori_protocol::Notice {
                message: "current session output".to_string(),
            },
        )),
    );

    let rendered = history_text(&mut rx);
    assert!(!rendered.contains("stale session output"), "{rendered}");
    assert!(rendered.contains("current session output"), "{rendered}");

    chat.handle_session_event(
        stale_generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::SessionEnded(
            nori_protocol::SessionEnded {
                reason: nori_protocol::SessionEndReason::Shutdown,
                message: None,
            },
        )),
    );

    let stale_terminal_events = std::iter::from_fn(|| rx.try_recv().ok()).collect::<Vec<_>>();
    assert!(
        stale_terminal_events
            .iter()
            .all(|event| !matches!(event, AppEvent::ExitRequest)),
        "a stale shutdown must not exit the replacement session: {stale_terminal_events:#?}"
    );

    chat.handle_session_event(
        current_generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::SessionEnded(
            nori_protocol::SessionEnded {
                reason: nori_protocol::SessionEndReason::Shutdown,
                message: None,
            },
        )),
    );
    let current_terminal_events = std::iter::from_fn(|| rx.try_recv().ok()).collect::<Vec<_>>();
    assert_eq!(
        current_terminal_events
            .iter()
            .filter(|event| matches!(event, AppEvent::ExitRequest))
            .count(),
        1,
        "the current shutdown must still exit: {current_terminal_events:#?}"
    );
}

#[tokio::test]
async fn session_shutdown_uses_live_handle_without_a_conversation_id() {
    let mut session = start_mock_session().await;

    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    assert_eq!(chat.conversation_id(), None);
    chat.harness_handle = Some(session.handle.clone());

    chat.shutdown_harness_session();

    let ended = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if let Some(nori_protocol::SessionEvent::Nori(
                nori_protocol::NoriEvent::SessionEnded(ended),
            )) = session.events.recv().await
            {
                break ended;
            }
        }
    })
    .await
    .expect("shutdown should reach the live harness session");
    assert_eq!(ended.reason, nori_protocol::SessionEndReason::Shutdown);
}

/// A transient (retryable) turn failure must leave the loop armed: the next
/// iteration fires when the turn completes.
#[test]
fn loop_survives_retryable_failure() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.first_prompt_text = Some("hi".to_string());
    chat.loop_remaining = Some(5);
    chat.loop_total = Some(10);

    deliver_completion(&mut chat, Some(crate::presentation::TurnFailure::Retryable));

    assert_eq!(next_loop_iteration(&mut rx), Some((4, 10)));
}

/// A fatal turn failure disarms the loop before it can re-fire.
#[test]
fn loop_stops_on_fatal_failure() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.first_prompt_text = Some("hi".to_string());
    chat.loop_remaining = Some(5);
    chat.loop_total = Some(10);

    deliver_completion(&mut chat, Some(crate::presentation::TurnFailure::Fatal));

    assert_eq!(chat.loop_remaining, None);
    assert_eq!(next_loop_iteration(&mut rx), None);
}

#[test]
fn raw_retryable_prompt_error_completes_and_retries_the_loop_once() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.first_prompt_text = Some("hi".to_string());
    chat.loop_remaining = Some(5);
    chat.loop_total = Some(10);

    deliver_prompt_failure(
        &mut chat,
        nori_protocol::acp::v1::RequestId::Str("prompt-retryable".to_string()),
        nori_protocol::RequestFailureKind::Retryable,
        true,
    );

    let (loop_iterations, rendered) = drain_loop_iterations_and_history(&mut rx);
    assert_eq!(loop_iterations, vec![(4, 10)]);
    assert!(rendered.contains("classified prompt failure"), "{rendered}");
    assert!(!rendered.contains("raw ACP prompt error"), "{rendered}");
}

#[test]
fn raw_fatal_prompt_error_completes_and_disarms_the_loop() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.first_prompt_text = Some("hi".to_string());
    chat.loop_remaining = Some(5);
    chat.loop_total = Some(10);

    deliver_prompt_failure(
        &mut chat,
        nori_protocol::acp::v1::RequestId::Str("prompt-fatal".to_string()),
        nori_protocol::RequestFailureKind::Fatal,
        false,
    );

    let (loop_iterations, rendered) = drain_loop_iterations_and_history(&mut rx);
    assert_eq!(chat.loop_remaining, None);
    assert!(loop_iterations.is_empty(), "{loop_iterations:?}");
    assert!(rendered.contains("classified prompt failure"), "{rendered}");
    assert!(!rendered.contains("raw ACP prompt error"), "{rendered}");
}

#[test]
fn delayed_raw_errors_from_multiple_failed_prompts_are_not_rendered() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let generation = chat.session_generation;
    let request_ids = [
        nori_protocol::acp::v1::RequestId::Str("prompt-a".to_string()),
        nori_protocol::acp::v1::RequestId::Str("prompt-b".to_string()),
    ];

    for (index, request_id) in request_ids.iter().cloned().enumerate() {
        chat.handle_session_event(
            generation,
            nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::SessionPhaseChanged(
                nori_protocol::SessionPhase::Prompting {
                    request_id: request_id.clone(),
                },
            )),
        );
        chat.handle_session_event(
            generation,
            nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::RequestFailed(
                nori_protocol::RequestFailure {
                    request_id: Some(request_id),
                    message: format!("classified failure {index}"),
                    kind: nori_protocol::RequestFailureKind::Fatal,
                },
            )),
        );
    }

    for (index, request_id) in request_ids.into_iter().enumerate() {
        chat.handle_session_event(
            generation,
            nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Response {
                request_id,
                response: Err(nori_protocol::acp::v1::Error::new(
                    -32001,
                    format!("delayed raw error {index}"),
                )),
            }),
        );
    }

    let rendered = history_text(&mut rx);
    assert!(rendered.contains("classified failure 0"), "{rendered}");
    assert!(rendered.contains("classified failure 1"), "{rendered}");
    assert!(!rendered.contains("delayed raw error"), "{rendered}");
}

#[test]
fn unrelated_request_failure_does_not_complete_the_active_prompt() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.first_prompt_text = Some("hi".to_string());
    chat.loop_remaining = Some(5);
    chat.loop_total = Some(10);
    let generation = chat.session_generation;
    chat.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::SessionPhaseChanged(
            nori_protocol::SessionPhase::Prompting {
                request_id: nori_protocol::acp::v1::RequestId::Str("active-prompt".to_string()),
            },
        )),
    );
    assert!(chat.bottom_pane.is_task_running());

    chat.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::RequestFailed(
            nori_protocol::RequestFailure {
                request_id: Some(nori_protocol::acp::v1::RequestId::Str(
                    "unrelated-request".to_string(),
                )),
                message: "unrelated failure".to_string(),
                kind: nori_protocol::RequestFailureKind::Fatal,
            },
        )),
    );

    assert!(chat.bottom_pane.is_task_running());
    assert_eq!(chat.loop_remaining, Some(5));
    assert_eq!(next_loop_iteration(&mut rx), None);
}

#[test]
fn only_the_correlated_successful_prompt_response_completes_the_turn() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.first_prompt_text = Some("hi".to_string());
    chat.loop_remaining = Some(5);
    chat.loop_total = Some(10);
    let generation = chat.session_generation;
    let active_request_id = nori_protocol::acp::v1::RequestId::Str("active-prompt".to_string());
    chat.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::SessionPhaseChanged(
            nori_protocol::SessionPhase::Prompting {
                request_id: active_request_id.clone(),
            },
        )),
    );
    chat.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Response {
            request_id: nori_protocol::acp::v1::RequestId::Str("unrelated-request".to_string()),
            response: Ok(nori_protocol::acp::v1::AgentResponse::PromptResponse(
                nori_protocol::acp::v1::PromptResponse::new(
                    nori_protocol::acp::v1::StopReason::EndTurn,
                ),
            )),
        }),
    );
    assert_eq!(next_loop_iteration(&mut rx), None);

    chat.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Response {
            request_id: active_request_id,
            response: Ok(nori_protocol::acp::v1::AgentResponse::PromptResponse(
                nori_protocol::acp::v1::PromptResponse::new(
                    nori_protocol::acp::v1::StopReason::EndTurn,
                ),
            )),
        }),
    );
    assert_eq!(next_loop_iteration(&mut rx), Some((4, 10)));
}

/// A turn that ended in a failure must not also render the generic
/// "Conversation interrupted" cell — the surfaced error already explains it.
#[test]
fn failure_completion_does_not_add_interrupted_cell() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    deliver_completion(&mut chat, Some(crate::presentation::TurnFailure::Fatal));

    assert!(
        !history_text(&mut rx).contains("Conversation interrupted"),
        "a failure completion should not add the interrupted cell"
    );
}

/// A genuine user cancellation (no failure) still renders the interrupted cell.
#[test]
fn user_cancellation_adds_interrupted_cell() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    deliver_completion(&mut chat, None);

    assert!(
        history_text(&mut rx).contains("Conversation interrupted"),
        "a clean user cancellation should add the interrupted cell"
    );
}

/// Launch a live mock-agent session and wait for it to start, returning its
/// handle for driving session-config operations.
async fn start_mock_session() -> nori_harness::runtime::LaunchedSession {
    let temp = tempfile::tempdir().expect("create session directory");
    let config = nori_config::NoriConfig {
        active_agent: "mock-model".to_string(),
        cwd: temp.path().to_path_buf(),
        nori_home: temp.path().to_path_buf(),
        ..Default::default()
    };
    let agent = nori_harness::runtime::prepare_agent(nori_harness::runtime::AgentPrepareSpec {
        config: std::sync::Arc::new(config),
        cli_version: "tui-test".to_string(),
        session_context: None,
        initial_context: None,
    })
    .await
    .expect("prepare test agent");
    let mut session =
        nori_harness::runtime::launch_session(nori_harness::runtime::SessionLaunchSpec {
            agent,
            start: nori_harness::runtime::SessionStart::New,
        });
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while !matches!(
            session.events.recv().await,
            Some(nori_protocol::SessionEvent::Nori(
                nori_protocol::NoriEvent::SessionStarted(_)
            ))
        ) {}
    })
    .await
    .expect("mock session should start");
    session
}

/// Selecting a value in the `/config` picker returns the user to the config
/// panel with the just-edited option selected.
#[tokio::test]
async fn config_value_selection_reopens_panel_focused_on_edited_option() {
    let session = start_mock_session().await;
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.harness_handle = Some(session.handle.clone());

    chat.set_acp_session_config_option(
        "thought_level".to_string(),
        "high".to_string(),
        "Thought Level".to_string(),
        "High".to_string(),
        false,
    );

    let focus = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if let Some(AppEvent::OpenAcpSessionConfigPicker {
                focus_config_id, ..
            }) = rx.recv().await
            {
                break focus_config_id;
            }
        }
    })
    .await
    .expect("the config panel should reopen after a value is chosen");

    assert_eq!(focus, Some("thought_level".to_string()));
}

/// Cycling the agent mode via its hotkey performs a real config set but must
/// NOT pop the `/config` panel open (that reopen is exclusive to the picker).
#[tokio::test]
async fn mode_cycle_does_not_reopen_config_panel() {
    let session = start_mock_session().await;
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.harness_handle = Some(session.handle.clone());

    // Seed a Mode-category option the mock accepts as settable so cycling
    // actually performs a set instead of the no-op "no mode available" branch.
    let mode_option = nori_protocol::acp::v1::SessionConfigOption::select(
        "thought_level",
        "Thought Level",
        "medium",
        vec![
            nori_protocol::acp::v1::SessionConfigSelectOption::new("low", "Low"),
            nori_protocol::acp::v1::SessionConfigSelectOption::new("medium", "Medium"),
            nori_protocol::acp::v1::SessionConfigSelectOption::new("high", "High"),
        ],
    )
    .category(nori_protocol::acp::v1::SessionConfigOptionCategory::Mode);
    chat.acp_mode_config =
        crate::nori::session_config_mode::acp_mode_config_from_options(&[mode_option]);
    assert!(
        chat.acp_mode_config.is_some(),
        "seeded mode config should parse"
    );

    chat.cycle_acp_mode_config();

    // Stream events through the successful set result and assert that no
    // config-panel reopen was requested along the way.
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            match rx.recv().await {
                Some(AppEvent::AcpSessionConfigSetResult { success, .. }) => {
                    assert!(success, "mode cycle set should succeed against the mock");
                    break;
                }
                Some(AppEvent::OpenAcpSessionConfigPicker { .. }) => {
                    panic!("cycling mode must not reopen the /config panel");
                }
                Some(_) => continue,
                None => panic!("event channel closed before the set completed"),
            }
        }
    })
    .await
    .expect("mode cycle should complete");
}

#[test]
fn session_forked_shows_resume_hint_for_previous_conversation() {
    let (mut chat, mut rx, _unused) = make_chatwidget_manual();
    let generation = chat.session_generation;

    chat.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::SessionForked(
            nori_protocol::SessionForked {
                previous_conversation_id: "11111111-1111-1111-1111-111111111111".to_string(),
                new_conversation_id: "22222222-2222-2222-2222-222222222222".to_string(),
                new_acp_session_id: nori_protocol::acp::v1::SessionId::from(
                    "acp-forked".to_string(),
                ),
            },
        )),
    );

    let rendered = history_text(&mut rx);
    assert!(
        rendered.contains("Session forked. To resume previous:"),
        "fork should add the resume-hint cell, got:\n{rendered}"
    );
    assert!(
        rendered.contains("nori resume 11111111-1111-1111-1111-111111111111"),
        "fork hint should offer a copy-pasteable resume command for the previous conversation, got:\n{rendered}"
    );
}

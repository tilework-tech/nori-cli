use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unified_exec_streams_after_lagged_output() -> Result<()> {
    skip_if_no_network!(Ok(()));
    skip_if_sandbox!(Ok(()));

    let server = start_mock_server().await;

    let mut builder = test_codex().with_config(|config| {
        config.use_experimental_unified_exec_tool = true;
        config.features.enable(Feature::UnifiedExec);
    });
    let TestCodex {
        codex,
        cwd,
        session_configured,
        ..
    } = builder.build(&server).await?;

    let script = r#"python3 - <<'PY'
import sys
import time

chunk = b'long content here to trigger truncation' * (1 << 10)
for _ in range(4):
    sys.stdout.buffer.write(chunk)
    sys.stdout.flush()

time.sleep(0.2)
for _ in range(5):
    sys.stdout.write("TAIL-MARKER\n")
    sys.stdout.flush()
    time.sleep(0.05)

time.sleep(0.2)
PY
"#;

    let first_call_id = "uexec-lag-start";
    let first_args = serde_json::json!({
        "cmd": script,
        "yield_time_ms": 25,
    });

    let second_call_id = "uexec-lag-poll";
    let second_args = serde_json::json!({
        "chars": "",
        "session_id": 1000,
        "yield_time_ms": 2_000,
    });

    let responses = vec![
        sse(vec![
            ev_response_created("resp-1"),
            ev_function_call(
                first_call_id,
                "exec_command",
                &serde_json::to_string(&first_args)?,
            ),
            ev_completed("resp-1"),
        ]),
        sse(vec![
            ev_response_created("resp-2"),
            ev_function_call(
                second_call_id,
                "write_stdin",
                &serde_json::to_string(&second_args)?,
            ),
            ev_completed("resp-2"),
        ]),
        sse(vec![
            ev_assistant_message("msg-1", "lag handled"),
            ev_completed("resp-3"),
        ]),
    ];
    mount_sse_sequence(&server, responses).await;

    let session_model = session_configured.model.clone();

    codex
        .submit(Op::UserTurn {
            items: vec![UserInput::Text {
                text: "exercise lag handling".into(),
            }],
            final_output_json_schema: None,
            cwd: cwd.path().to_path_buf(),
            approval_policy: AskForApproval::Never,
            sandbox_policy: SandboxPolicy::DangerFullAccess,
            model: session_model,
            effort: None,
            summary: ReasoningSummary::Auto,
        })
        .await?;
    // This is a worst case scenario for the truncate logic.
    wait_for_event_with_timeout(
        &codex,
        |event| matches!(event, EventMsg::TaskComplete(_)),
        Duration::from_secs(10),
    )
    .await;

    let requests = server.received_requests().await.expect("recorded requests");
    assert!(!requests.is_empty(), "expected at least one POST request");

    let bodies = requests
        .iter()
        .map(|req| req.body_json::<Value>().expect("request json"))
        .collect::<Vec<_>>();

    let outputs = collect_tool_outputs(&bodies)?;

    let start_output = outputs
        .get(first_call_id)
        .expect("missing initial unified_exec output");
    let process_id = start_output.process_id.clone().unwrap_or_default();
    assert!(
        !process_id.is_empty(),
        "expected session id from initial unified_exec response"
    );

    let poll_output = outputs
        .get(second_call_id)
        .expect("missing poll unified_exec output");
    let poll_text = poll_output.output.as_str();
    assert!(
        poll_text.contains("TAIL-MARKER"),
        "expected poll output to contain tail marker, got {poll_text:?}"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unified_exec_timeout_and_followup_poll() -> Result<()> {
    skip_if_no_network!(Ok(()));
    skip_if_sandbox!(Ok(()));

    let server = start_mock_server().await;

    let mut builder = test_codex().with_config(|config| {
        config.features.enable(Feature::UnifiedExec);
    });
    let TestCodex {
        codex,
        cwd,
        session_configured,
        ..
    } = builder.build(&server).await?;

    let first_call_id = "uexec-timeout";
    let first_args = serde_json::json!({
        "cmd": "sleep 0.5; echo ready",
        "yield_time_ms": 10,
    });

    let second_call_id = "uexec-poll";
    let second_args = serde_json::json!({
        "chars": "",
        "session_id": 1000,
        "yield_time_ms": 800,
    });

    let responses = vec![
        sse(vec![
            ev_response_created("resp-1"),
            ev_function_call(
                first_call_id,
                "exec_command",
                &serde_json::to_string(&first_args)?,
            ),
            ev_completed("resp-1"),
        ]),
        sse(vec![
            ev_response_created("resp-2"),
            ev_function_call(
                second_call_id,
                "write_stdin",
                &serde_json::to_string(&second_args)?,
            ),
            ev_completed("resp-2"),
        ]),
        sse(vec![
            ev_assistant_message("msg-1", "done"),
            ev_completed("resp-3"),
        ]),
    ];
    mount_sse_sequence(&server, responses).await;

    let session_model = session_configured.model.clone();

    codex
        .submit(Op::UserTurn {
            items: vec![UserInput::Text {
                text: "check timeout".into(),
            }],
            final_output_json_schema: None,
            cwd: cwd.path().to_path_buf(),
            approval_policy: AskForApproval::Never,
            sandbox_policy: SandboxPolicy::DangerFullAccess,
            model: session_model,
            effort: None,
            summary: ReasoningSummary::Auto,
        })
        .await?;

    loop {
        let event = codex.next_event().await.expect("event");
        if matches!(event.msg, EventMsg::TaskComplete(_)) {
            break;
        }
    }

    let requests = server.received_requests().await.expect("recorded requests");
    assert!(!requests.is_empty(), "expected at least one POST request");

    let bodies = requests
        .iter()
        .map(|req| req.body_json::<Value>().expect("request json"))
        .collect::<Vec<_>>();

    let outputs = collect_tool_outputs(&bodies)?;

    let first_output = outputs.get(first_call_id).expect("missing timeout output");
    assert!(first_output.process_id.is_some());
    assert!(first_output.output.is_empty());

    let poll_output = outputs.get(second_call_id).expect("missing poll output");
    let output_text = poll_output.output.as_str();
    assert!(
        output_text.contains("ready"),
        "expected ready output, got {output_text:?}"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// Skipped on arm because the ctor logic to handle arg0 doesn't work on ARM
#[cfg(not(target_arch = "arm"))]
async fn unified_exec_formats_large_output_summary() -> Result<()> {
    skip_if_no_network!(Ok(()));
    skip_if_sandbox!(Ok(()));

    let server = start_mock_server().await;

    let mut builder = test_codex().with_config(|config| {
        config.features.enable(Feature::UnifiedExec);
    });
    let TestCodex {
        codex,
        cwd,
        session_configured,
        ..
    } = builder.build(&server).await?;

    let script = r#"python3 - <<'PY'
import sys
sys.stdout.write("token token \n" * 5000)
PY
"#;

    let call_id = "uexec-large-output";
    let args = serde_json::json!({
        "cmd": script,
        "max_output_tokens": 100,
        "yield_time_ms": 500,
    });

    let responses = vec![
        sse(vec![
            ev_response_created("resp-1"),
            ev_function_call(call_id, "exec_command", &serde_json::to_string(&args)?),
            ev_completed("resp-1"),
        ]),
        sse(vec![
            ev_assistant_message("msg-1", "done"),
            ev_completed("resp-2"),
        ]),
    ];
    mount_sse_sequence(&server, responses).await;

    let session_model = session_configured.model.clone();

    codex
        .submit(Op::UserTurn {
            items: vec![UserInput::Text {
                text: "summarize large output".into(),
            }],
            final_output_json_schema: None,
            cwd: cwd.path().to_path_buf(),
            approval_policy: AskForApproval::Never,
            sandbox_policy: SandboxPolicy::DangerFullAccess,
            model: session_model,
            effort: None,
            summary: ReasoningSummary::Auto,
        })
        .await?;

    wait_for_event(&codex, |event| matches!(event, EventMsg::TaskComplete(_))).await;

    let requests = server.received_requests().await.expect("recorded requests");
    assert!(!requests.is_empty(), "expected at least one POST request");

    let bodies = requests
        .iter()
        .map(|req| req.body_json::<Value>().expect("request json"))
        .collect::<Vec<_>>();

    let outputs = collect_tool_outputs(&bodies)?;
    let large_output = outputs.get(call_id).expect("missing large output summary");

    let output_text = large_output.output.replace("\r\n", "\n");
    let truncated_pattern = r"(?s)^Total output lines: \d+\n\n(token token \n){5,}.*…\d+ tokens truncated….*(token token \n){5,}$";
    assert_regex_match(truncated_pattern, &output_text);

    let original_tokens = large_output
        .original_token_count
        .expect("missing original_token_count for large output summary");
    assert!(original_tokens > 0);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unified_exec_runs_under_sandbox() -> Result<()> {
    skip_if_no_network!(Ok(()));
    skip_if_sandbox!(Ok(()));

    let server = start_mock_server().await;

    let mut builder = test_codex().with_config(|config| {
        config.features.enable(Feature::UnifiedExec);
    });
    let TestCodex {
        codex,
        cwd,
        session_configured,
        ..
    } = builder.build(&server).await?;

    let call_id = "uexec";
    let args = serde_json::json!({
        "cmd": "echo 'hello'",
        "yield_time_ms": 500,
    });

    let responses = vec![
        sse(vec![
            ev_response_created("resp-1"),
            ev_function_call(call_id, "exec_command", &serde_json::to_string(&args)?),
            ev_completed("resp-1"),
        ]),
        sse(vec![
            ev_assistant_message("msg-1", "done"),
            ev_completed("resp-2"),
        ]),
    ];
    mount_sse_sequence(&server, responses).await;

    let session_model = session_configured.model.clone();

    codex
        .submit(Op::UserTurn {
            items: vec![UserInput::Text {
                text: "summarize large output".into(),
            }],
            final_output_json_schema: None,
            cwd: cwd.path().to_path_buf(),
            approval_policy: AskForApproval::Never,
            // Important!
            sandbox_policy: SandboxPolicy::ReadOnly,
            model: session_model,
            effort: None,
            summary: ReasoningSummary::Auto,
        })
        .await?;

    wait_for_event(&codex, |event| matches!(event, EventMsg::TaskComplete(_))).await;

    let requests = server.received_requests().await.expect("recorded requests");
    assert!(!requests.is_empty(), "expected at least one POST request");

    let bodies = requests
        .iter()
        .map(|req| req.body_json::<Value>().expect("request json"))
        .collect::<Vec<_>>();

    let outputs = collect_tool_outputs(&bodies)?;
    let output = outputs.get(call_id).expect("missing output");

    assert_regex_match("hello[\r\n]+", &output.output);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn unified_exec_prunes_exited_sessions_first() -> Result<()> {
    skip_if_no_network!(Ok(()));
    skip_if_sandbox!(Ok(()));

    let server = start_mock_server().await;

    let mut builder = test_codex().with_config(|config| {
        config.use_experimental_unified_exec_tool = true;
        config.features.enable(Feature::UnifiedExec);
    });
    let TestCodex {
        codex,
        cwd,
        session_configured,
        ..
    } = builder.build(&server).await?;

    const MAX_SESSIONS_FOR_TEST: i32 = 64;
    const FILLER_SESSIONS: i32 = MAX_SESSIONS_FOR_TEST - 1;

    let keep_call_id = "uexec-prune-keep";
    let keep_args = serde_json::json!({
        "cmd": "/bin/cat",
        "yield_time_ms": 250,
    });

    let prune_call_id = "uexec-prune-target";
    // Give the sleeper time to exit before the filler sessions trigger pruning.
    let prune_args = serde_json::json!({
        "cmd": "sleep 1",
        "yield_time_ms": 1_250,
    });

    let mut events = vec![ev_response_created("resp-prune-1")];
    events.push(ev_function_call(
        keep_call_id,
        "exec_command",
        &serde_json::to_string(&keep_args)?,
    ));
    events.push(ev_function_call(
        prune_call_id,
        "exec_command",
        &serde_json::to_string(&prune_args)?,
    ));

    for idx in 0..FILLER_SESSIONS {
        let filler_args = serde_json::json!({
            "cmd": format!("echo filler {idx}"),
            "yield_time_ms": 250,
        });
        let call_id = format!("uexec-prune-fill-{idx}");
        events.push(ev_function_call(
            &call_id,
            "exec_command",
            &serde_json::to_string(&filler_args)?,
        ));
    }

    let keep_write_call_id = "uexec-prune-keep-write";
    let keep_write_args = serde_json::json!({
        "chars": "still alive\n",
        "session_id": 1000,
        "yield_time_ms": 500,
    });
    events.push(ev_function_call(
        keep_write_call_id,
        "write_stdin",
        &serde_json::to_string(&keep_write_args)?,
    ));

    let probe_call_id = "uexec-prune-probe";
    let probe_args = serde_json::json!({
        "chars": "should fail\n",
        "session_id": 1001,
        "yield_time_ms": 500,
    });
    events.push(ev_function_call(
        probe_call_id,
        "write_stdin",
        &serde_json::to_string(&probe_args)?,
    ));

    events.push(ev_completed("resp-prune-1"));
    let first_response = sse(events);
    let completion_response = sse(vec![
        ev_response_created("resp-prune-2"),
        ev_assistant_message("msg-prune", "done"),
        ev_completed("resp-prune-2"),
    ]);
    let response_mock =
        mount_sse_sequence(&server, vec![first_response, completion_response]).await;

    let session_model = session_configured.model.clone();

    codex
        .submit(Op::UserTurn {
            items: vec![UserInput::Text {
                text: "fill session cache".into(),
            }],
            final_output_json_schema: None,
            cwd: cwd.path().to_path_buf(),
            approval_policy: AskForApproval::Never,
            sandbox_policy: SandboxPolicy::DangerFullAccess,
            model: session_model,
            effort: None,
            summary: ReasoningSummary::Auto,
        })
        .await?;

    wait_for_event(&codex, |event| matches!(event, EventMsg::TaskComplete(_))).await;

    let requests = response_mock.requests();
    assert!(
        !requests.is_empty(),
        "expected at least one response request"
    );

    let keep_start = requests
        .iter()
        .find_map(|req| req.function_call_output_text(keep_call_id))
        .expect("missing initial keep session output");
    let keep_start_output = parse_unified_exec_output(&keep_start)?;
    assert!(keep_start_output.process_id.is_some());
    assert!(keep_start_output.exit_code.is_none());

    let prune_start = requests
        .iter()
        .find_map(|req| req.function_call_output_text(prune_call_id))
        .expect("missing initial prune session output");
    let prune_start_output = parse_unified_exec_output(&prune_start)?;
    assert!(prune_start_output.process_id.is_some());
    assert!(prune_start_output.exit_code.is_none());

    let keep_write = requests
        .iter()
        .find_map(|req| req.function_call_output_text(keep_write_call_id))
        .expect("missing keep write output");
    let keep_write_output = parse_unified_exec_output(&keep_write)?;
    assert!(keep_write_output.process_id.is_some());
    assert!(
        keep_write_output.output.contains("still alive"),
        "expected cat session to echo input, got {:?}",
        keep_write_output.output
    );

    let pruned_probe = requests
        .iter()
        .find_map(|req| req.function_call_output_text(probe_call_id))
        .expect("missing probe output");
    assert!(
        pruned_probe.contains("UnknownSessionId") || pruned_probe.contains("Unknown process id"),
        "expected probe to fail after pruning, got {pruned_probe:?}"
    );

    Ok(())
}

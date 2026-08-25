use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

#[cfg(unix)]
use nori_config::AskForApproval;
use nori_config::NoriConfig;
use nori_harness::runtime::AgentPrepareSpec;
use nori_harness::runtime::LaunchedSession;
use nori_harness::runtime::SessionCatalog;
use nori_harness::runtime::SessionLaunchSpec;
use nori_harness::runtime::SessionResume;
use nori_harness::runtime::SessionStart;
use nori_harness::runtime::launch_session;
use nori_harness::runtime::prepare_agent;
use nori_harness::runtime::refresh_prepared_agent;
use nori_protocol::AcpEvent;
use nori_protocol::NoriEvent;
use nori_protocol::SessionEvent;
use pretty_assertions::assert_eq;
use serial_test::serial;

struct EnvGuard(&'static str);

impl Drop for EnvGuard {
    fn drop(&mut self) {
        // SAFETY: environment-mutating mock-agent tests run serially.
        unsafe { std::env::remove_var(self.0) };
    }
}

fn prepare_spec(cwd: &Path, wire_log_dir: PathBuf) -> AgentPrepareSpec {
    let config = NoriConfig {
        active_agent: "mock-model".to_string(),
        cwd: cwd.to_path_buf(),
        nori_home: cwd.to_path_buf(),
        acp_proxy: nori_config::AcpProxyConfig {
            enabled: true,
            log_dir: wire_log_dir,
        },
        ..Default::default()
    };
    AgentPrepareSpec {
        config: Arc::new(config),
        cli_version: "prepared-agent-test".to_string(),
        session_context: None,
        initial_context: None,
    }
}

#[expect(
    clippy::expect_used,
    reason = "test helper should fail at the malformed ACP wire-log boundary"
)]
fn recorded_client_methods(wire_log_dir: &Path) -> Vec<String> {
    let wire_logs = std::fs::read_dir(wire_log_dir)
        .expect("wire log directory")
        .map(|entry| entry.expect("wire log entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "jsonl")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        wire_logs.len(),
        1,
        "preparation and activation must use one ACP subprocess"
    );

    std::fs::read_to_string(&wire_logs[0])
        .expect("read ACP wire log")
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("valid wire record"))
        .filter(|record| record["direction"] == "client_to_agent")
        .filter_map(|record| record["message"]["method"].as_str().map(str::to_string))
        .collect()
}

#[expect(
    clippy::expect_used,
    reason = "wire-log filenames are the process-boundary test fixture"
)]
#[cfg(unix)]
fn recorded_agent_pid(wire_log_dir: &Path) -> u32 {
    let wire_log = std::fs::read_dir(wire_log_dir)
        .expect("wire log directory")
        .next()
        .expect("wire log")
        .expect("wire log entry")
        .path();
    wire_log
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.split('-').nth(1))
        .and_then(|pid| pid.parse::<u32>().ok())
        .expect("wire log filename contains child pid")
}

#[expect(
    clippy::expect_used,
    reason = "test helper should fail at the session-start boundary"
)]
async fn wait_for_started(session: &mut LaunchedSession) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match session.events.recv().await.expect("session event stream") {
                SessionEvent::Nori(NoriEvent::SessionStarted(_)) => return,
                SessionEvent::Nori(NoriEvent::SessionEnded(ended)) => {
                    panic!("session ended before startup: {ended:?}")
                }
                SessionEvent::Acp(_) | SessionEvent::Nori(_) => {}
            }
        }
    })
    .await
    .expect("prepared session should start");
}

#[tokio::test]
#[serial]
async fn listed_agent_is_activated_on_the_same_connection() {
    // SAFETY: environment-mutating mock-agent tests run serially.
    unsafe { std::env::set_var("MOCK_AGENT_SUPPORT_SESSION_LIST", "1") };
    let _guard = EnvGuard("MOCK_AGENT_SUPPORT_SESSION_LIST");
    let temp = tempfile::tempdir().expect("create test directory");
    let wire_log_dir = temp.path().join("wire-logs");

    let prepared = prepare_agent(prepare_spec(temp.path(), wire_log_dir.clone()))
        .await
        .expect("prepare agent");

    assert!(
        matches!(prepared.catalog(), SessionCatalog::Listed(sessions) if sessions.len() == 2),
        "an advertised successful list must remain distinct from unsupported listing"
    );

    assert_eq!(
        recorded_client_methods(&wire_log_dir),
        vec!["initialize", "session/list"],
        "preparation must inspect sessions without creating one"
    );

    let mut session = launch_session(SessionLaunchSpec {
        agent: prepared,
        start: SessionStart::New,
    });
    wait_for_started(&mut session).await;

    assert_eq!(
        recorded_client_methods(&wire_log_dir),
        vec!["initialize", "session/list", "session/new"],
        "activation must continue on the prepared connection"
    );
    session.handle.shutdown().await.expect("shutdown session");
}

#[tokio::test]
#[serial]
async fn agent_without_session_list_is_prepared_before_new_session() {
    // SAFETY: environment-mutating mock-agent tests run serially.
    unsafe { std::env::remove_var("MOCK_AGENT_SUPPORT_SESSION_LIST") };
    let _guard = EnvGuard("MOCK_AGENT_SUPPORT_SESSION_LIST");
    let temp = tempfile::tempdir().expect("create test directory");
    let wire_log_dir = temp.path().join("wire-logs");

    let prepared = prepare_agent(prepare_spec(temp.path(), wire_log_dir.clone()))
        .await
        .expect("prepare agent without session/list");
    assert_eq!(prepared.catalog(), &SessionCatalog::Unsupported);
    assert_eq!(recorded_client_methods(&wire_log_dir), vec!["initialize"]);

    let mut session = launch_session(SessionLaunchSpec {
        agent: prepared,
        start: SessionStart::New,
    });
    wait_for_started(&mut session).await;

    assert_eq!(
        recorded_client_methods(&wire_log_dir),
        vec!["initialize", "session/new"]
    );
    session.handle.shutdown().await.expect("shutdown session");
}

#[tokio::test]
#[serial]
async fn listed_agent_loads_on_the_same_connection() {
    // SAFETY: environment-mutating mock-agent tests run serially.
    unsafe {
        std::env::set_var("MOCK_AGENT_SUPPORT_SESSION_LIST", "1");
        std::env::set_var("MOCK_AGENT_SUPPORT_LOAD_SESSION", "1");
    }
    let _list_guard = EnvGuard("MOCK_AGENT_SUPPORT_SESSION_LIST");
    let _load_guard = EnvGuard("MOCK_AGENT_SUPPORT_LOAD_SESSION");
    let temp = tempfile::tempdir().expect("create test directory");
    let wire_log_dir = temp.path().join("wire-logs");

    let prepared = prepare_agent(prepare_spec(temp.path(), wire_log_dir.clone()))
        .await
        .expect("prepare agent");
    let mut session = launch_session(SessionLaunchSpec {
        agent: prepared,
        start: SessionStart::Resume(SessionResume {
            acp_session_id: Some("mock-session-1".to_string()),
            transcript: None,
        }),
    });
    wait_for_started(&mut session).await;

    assert_eq!(
        recorded_client_methods(&wire_log_dir),
        vec!["initialize", "session/list", "session/load"],
        "load must consume the connection that produced the selected catalog row"
    );
    session.handle.shutdown().await.expect("shutdown session");
}

#[tokio::test]
#[serial]
#[cfg(unix)]
async fn dropping_prepared_agent_reaps_its_process() {
    // SAFETY: environment-mutating mock-agent tests run serially.
    unsafe { std::env::set_var("MOCK_AGENT_SUPPORT_SESSION_LIST", "1") };
    let _guard = EnvGuard("MOCK_AGENT_SUPPORT_SESSION_LIST");
    let temp = tempfile::tempdir().expect("create test directory");
    let wire_log_dir = temp.path().join("wire-logs");

    let prepared = prepare_agent(prepare_spec(temp.path(), wire_log_dir.clone()))
        .await
        .expect("prepare agent");
    let pid = recorded_agent_pid(&wire_log_dir);

    drop(prepared);

    tokio::time::timeout(Duration::from_secs(5), async {
        while unsafe { libc::kill(pid as i32, 0) } == 0 {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("dropping an unused prepared agent must reap its subprocess");
}

#[tokio::test]
#[serial]
#[cfg(unix)]
async fn shutting_down_prepared_agent_promptly_reaps_an_eof_ignoring_child() {
    // SAFETY: environment-mutating mock-agent tests run serially.
    unsafe {
        std::env::set_var("MOCK_AGENT_SUPPORT_SESSION_LIST", "1");
        std::env::set_var("MOCK_AGENT_IGNORE_EOF", "1");
    }
    let _list_guard = EnvGuard("MOCK_AGENT_SUPPORT_SESSION_LIST");
    let _eof_guard = EnvGuard("MOCK_AGENT_IGNORE_EOF");
    let temp = tempfile::tempdir().expect("create test directory");
    let wire_log_dir = temp.path().join("wire-logs");
    let prepared = prepare_agent(prepare_spec(temp.path(), wire_log_dir.clone()))
        .await
        .expect("prepare agent");
    let pid = recorded_agent_pid(&wire_log_dir);

    tokio::time::timeout(Duration::from_secs(3), prepared.shutdown())
        .await
        .expect("pre-session shutdown must not use the active-session detach grace");

    assert_ne!(
        unsafe { libc::kill(pid as i32, 0) },
        0,
        "prepared child should be gone when shutdown returns"
    );
}

#[tokio::test]
#[serial]
async fn advertised_session_list_failure_is_not_treated_as_unsupported() {
    // SAFETY: environment-mutating mock-agent tests run serially.
    unsafe {
        std::env::set_var("MOCK_AGENT_SUPPORT_SESSION_LIST", "1");
        std::env::set_var("MOCK_AGENT_LIST_SESSIONS_FAIL", "1");
    }
    let _support_guard = EnvGuard("MOCK_AGENT_SUPPORT_SESSION_LIST");
    let _failure_guard = EnvGuard("MOCK_AGENT_LIST_SESSIONS_FAIL");
    let temp = tempfile::tempdir().expect("create test directory");
    let wire_log_dir = temp.path().join("wire-logs");

    let error = prepare_agent(prepare_spec(temp.path(), wire_log_dir.clone()))
        .await
        .expect_err("advertised session/list failure must fail preparation");

    assert!(
        format!("{error:#}").contains("Failed to list ACP sessions"),
        "the list failure should remain the preparation error: {error:#}"
    );
    assert_eq!(
        recorded_client_methods(&wire_log_dir),
        vec!["initialize", "session/list"],
        "a list failure must not silently fall through to session/new"
    );
}

#[tokio::test]
#[serial]
#[cfg(unix)]
async fn activation_refreshes_mutable_policy_without_replacing_the_connection() {
    let temp = tempfile::tempdir().expect("create test directory");
    let wire_log_dir = temp.path().join("wire-logs");
    let mut prepared = prepare_agent(prepare_spec(temp.path(), wire_log_dir.clone()))
        .await
        .expect("prepare agent");
    let prepared_pid = recorded_agent_pid(&wire_log_dir);

    let mut latest_spec = prepare_spec(temp.path(), wire_log_dir.clone());
    Arc::make_mut(&mut latest_spec.config).approval_policy = AskForApproval::Never;
    refresh_prepared_agent(&mut prepared, latest_spec)
        .expect("refresh activation-time configuration");

    let mut session = launch_session(SessionLaunchSpec {
        agent: prepared,
        start: SessionStart::New,
    });
    wait_for_started(&mut session).await;
    let prompt_request_id = session
        .handle
        .prompt(vec![nori_protocol::acp::v1::ContentBlock::Text(
            nori_protocol::acp::v1::TextContent::new("mock:request-permission"),
        )])
        .await
        .expect("submit permission prompt");

    let confirmation = tokio::time::timeout(Duration::from_secs(10), async {
        let mut confirmation = None;
        loop {
            match session
                .events
                .recv()
                .await
                .expect("session event stream closed before prompt completion")
            {
                SessionEvent::Acp(AcpEvent::Request {
                    request: nori_protocol::acp::v1::AgentRequest::RequestPermissionRequest(_),
                    ..
                }) => panic!("latest never-ask policy should auto-approve the request"),
                SessionEvent::Acp(AcpEvent::Notification(
                    nori_protocol::acp::v1::AgentNotification::SessionNotification(notification),
                )) => {
                    if let nori_protocol::acp::v1::SessionUpdate::AgentMessageChunk(chunk) =
                        notification.update
                        && let nori_protocol::acp::v1::ContentBlock::Text(text) = chunk.content
                        && text.text.contains("Permission granted with option: allow")
                    {
                        confirmation = Some(text.text);
                    }
                }
                SessionEvent::Acp(AcpEvent::Response {
                    request_id,
                    response: Ok(nori_protocol::acp::v1::AgentResponse::PromptResponse(_)),
                }) if request_id == prompt_request_id => return confirmation,
                SessionEvent::Acp(_) | SessionEvent::Nori(_) => {}
            }
        }
    })
    .await
    .expect("refreshed approval policy should let the prompt finish");

    assert_eq!(
        confirmation.as_deref(),
        Some("Permission granted with option: allow")
    );
    let methods = recorded_client_methods(&wire_log_dir);
    assert!(methods.iter().any(|method| method == "session/new"));
    assert!(methods.iter().any(|method| method == "session/prompt"));
    assert_eq!(recorded_agent_pid(&wire_log_dir), prepared_pid);
    session.handle.shutdown().await.expect("shutdown session");
}

#[tokio::test]
#[serial]
async fn refresh_rejects_spawn_fixed_changes_without_destroying_the_prepared_agent() {
    let temp = tempfile::tempdir().expect("create test directory");
    let wire_log_dir = temp.path().join("wire-logs");
    let mut prepared = prepare_agent(prepare_spec(temp.path(), wire_log_dir.clone()))
        .await
        .expect("prepare agent");

    let mut identity_spec = prepare_spec(temp.path(), wire_log_dir.clone());
    Arc::make_mut(&mut identity_spec.config).active_agent = "different-agent".to_string();
    assert!(
        refresh_prepared_agent(&mut prepared, identity_spec).is_err(),
        "agent identity is fixed by preparation"
    );

    let mut cwd_spec = prepare_spec(temp.path(), wire_log_dir.clone());
    Arc::make_mut(&mut cwd_spec.config).cwd = temp.path().join("different-cwd");
    assert!(
        refresh_prepared_agent(&mut prepared, cwd_spec).is_err(),
        "working directory is fixed by preparation"
    );

    let mut proxy_spec = prepare_spec(temp.path(), wire_log_dir.clone());
    Arc::make_mut(&mut proxy_spec.config).acp_proxy.enabled = false;
    assert!(
        refresh_prepared_agent(&mut prepared, proxy_spec).is_err(),
        "wire recording is fixed by preparation"
    );

    let mut model_spec = prepare_spec(temp.path(), wire_log_dir.clone());
    Arc::make_mut(&mut model_spec.config)
        .default_models
        .insert("mock-model".to_string(), "custom-model".to_string());
    assert!(
        refresh_prepared_agent(&mut prepared, model_spec).is_err(),
        "spawn-time model injection is fixed by preparation"
    );

    let mut session = launch_session(SessionLaunchSpec {
        agent: prepared,
        start: SessionStart::New,
    });
    wait_for_started(&mut session).await;
    let methods = recorded_client_methods(&wire_log_dir);
    assert_eq!(methods.first().map(String::as_str), Some("initialize"));
    assert!(methods.iter().any(|method| method == "session/new"));
    session.handle.shutdown().await.expect("shutdown session");
}

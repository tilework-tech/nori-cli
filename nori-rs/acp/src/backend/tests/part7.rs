use pretty_assertions::assert_eq;

use super::*;

/// `[default_models]` entries must be applied to new sessions through the
/// stable `session/set_config_option` mechanism when the agent advertises a
/// Model-category config option. `mock-model-fast` is advertised as a config
/// option value, so the persisted default must reach the agent on the wire.
#[tokio::test]
#[serial]
async fn default_model_applied_via_stable_config_options_on_session_start() {
    let mock_config =
        crate::registry::get_agent_config("mock-model").expect("mock-model should be registered");
    if !std::path::Path::new(&mock_config.command).exists() {
        eprintln!(
            "Skipping test: mock_acp_agent not found at {}",
            mock_config.command
        );
        return;
    }

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let wire_log_dir = temp_dir.path().join("acp-wire");
    let (backend_event_tx, mut backend_event_rx) = mpsc::channel(64);
    let mut config = build_test_config(temp_dir.path());
    config.acp_proxy = crate::config::AcpProxyConfig {
        enabled: true,
        log_dir: wire_log_dir.clone(),
    };
    config.default_model = Some("mock-model-fast".to_string());

    let backend = AcpBackend::spawn(&config, backend_event_tx)
        .await
        .expect("Failed to spawn ACP backend");

    let _ = recv_backend_control(&mut backend_event_rx, std::time::Duration::from_secs(5))
        .await
        .expect("Should receive SessionConfigured event");

    let params = wait_for_logged_request(
        &wire_log_dir,
        "session/set_config_option",
        std::time::Duration::from_secs(5),
    )
    .await
    .expect("default model should be applied via session/set_config_option");
    assert_eq!(params["configId"], "model");
    assert_eq!(params["value"], "mock-model-fast");
    assert_eq!(
        count_logged_requests(&wire_log_dir, "session/set_model"),
        0,
        "stable config-option application must replace session/set_model, not double-apply"
    );

    backend
        .submit(Op::Shutdown)
        .await
        .expect("Failed to shut down ACP backend");
}

/// A persisted default model the agent no longer advertises must be skipped:
/// no model selection is sent on the wire, and the session still starts.
#[tokio::test]
#[serial]
async fn unknown_default_model_is_skipped_on_session_start() {
    let mock_config =
        crate::registry::get_agent_config("mock-model").expect("mock-model should be registered");
    if !std::path::Path::new(&mock_config.command).exists() {
        eprintln!(
            "Skipping test: mock_acp_agent not found at {}",
            mock_config.command
        );
        return;
    }

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let wire_log_dir = temp_dir.path().join("acp-wire");
    let (backend_event_tx, mut backend_event_rx) = mpsc::channel(64);
    let mut config = build_test_config(temp_dir.path());
    config.acp_proxy = crate::config::AcpProxyConfig {
        enabled: true,
        log_dir: wire_log_dir.clone(),
    };
    config.default_model = Some("model-that-no-longer-exists".to_string());

    let backend = AcpBackend::spawn(&config, backend_event_tx)
        .await
        .expect("Failed to spawn ACP backend");

    let _ = recv_backend_control(&mut backend_event_rx, std::time::Duration::from_secs(5))
        .await
        .expect("Should receive SessionConfigured event");

    // Anchor the absence assertion to a completed prompt round-trip: once
    // session/prompt shows up in the wire log, session startup (where default
    // model application happens) is long past and the session is usable.
    backend
        .submit(Op::UserInput {
            items: vec![codex_protocol::user_input::UserInput::Text {
                text: "hello".to_string(),
            }],
        })
        .await
        .expect("Failed to submit prompt");
    wait_for_logged_request(
        &wire_log_dir,
        "session/prompt",
        std::time::Duration::from_secs(5),
    )
    .await
    .expect("session should accept prompts after skipping the unknown default model");

    assert_eq!(
        try_latest_logged_request_params(&wire_log_dir, "session/set_config_option"),
        None,
        "unknown default model must not be applied via config options"
    );
    assert_eq!(
        try_latest_logged_request_params(&wire_log_dir, "session/set_model"),
        None,
        "unknown default model must not be applied via session/set_model"
    );

    backend
        .submit(Op::Shutdown)
        .await
        .expect("Failed to shut down ACP backend");
}

/// The client-side replay fallback of `resume_session` creates a fresh
/// session, so the persisted default model must be applied to it just like a
/// regular spawn.
#[tokio::test]
#[serial]
async fn resume_replay_fallback_applies_default_model() {
    let mock_config =
        crate::registry::get_agent_config("mock-model").expect("mock-model should be registered");
    if !std::path::Path::new(&mock_config.command).exists() {
        eprintln!(
            "Skipping test: mock_acp_agent not found at {}",
            mock_config.command
        );
        return;
    }

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let wire_log_dir = temp_dir.path().join("acp-wire");
    let (backend_event_tx, mut backend_event_rx) = mpsc::channel(64);
    let mut config = build_test_config(temp_dir.path());
    config.acp_proxy = crate::config::AcpProxyConfig {
        enabled: true,
        log_dir: wire_log_dir.clone(),
    };
    config.default_model = Some("mock-model-fast".to_string());
    let transcript = build_test_transcript();

    // No ACP session id: the agent does not support session/load, forcing the
    // client-side replay fallback that creates a fresh session.
    let backend = AcpBackend::resume_session(&config, None, Some(&transcript), backend_event_tx)
        .await
        .expect("resume_session should succeed");

    let _ = recv_backend_control(&mut backend_event_rx, std::time::Duration::from_secs(5))
        .await
        .expect("Should receive SessionConfigured event");

    let params = wait_for_logged_request(
        &wire_log_dir,
        "session/set_config_option",
        std::time::Duration::from_secs(5),
    )
    .await
    .expect("default model should be applied to the fallback session");
    assert_eq!(params["configId"], "model");
    assert_eq!(params["value"], "mock-model-fast");

    backend
        .submit(Op::Shutdown)
        .await
        .expect("Failed to shut down ACP backend");
}

/// When the agent advertises a Model-category config option, that option owns
/// model selection exclusively: a persisted value it does not advertise is
/// skipped entirely, even if the unstable model API would accept it.
/// `mock-model-powerful` exists only in the unstable `available_models`.
#[tokio::test]
#[serial]
async fn stable_model_option_owns_selection_even_when_value_unknown() {
    let mock_config =
        crate::registry::get_agent_config("mock-model").expect("mock-model should be registered");
    if !std::path::Path::new(&mock_config.command).exists() {
        eprintln!(
            "Skipping test: mock_acp_agent not found at {}",
            mock_config.command
        );
        return;
    }

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let wire_log_dir = temp_dir.path().join("acp-wire");
    let (backend_event_tx, mut backend_event_rx) = mpsc::channel(64);
    let mut config = build_test_config(temp_dir.path());
    config.acp_proxy = crate::config::AcpProxyConfig {
        enabled: true,
        log_dir: wire_log_dir.clone(),
    };
    config.default_model = Some("mock-model-powerful".to_string());

    let backend = AcpBackend::spawn(&config, backend_event_tx)
        .await
        .expect("Failed to spawn ACP backend");

    let _ = recv_backend_control(&mut backend_event_rx, std::time::Duration::from_secs(5))
        .await
        .expect("Should receive SessionConfigured event");

    backend
        .submit(Op::UserInput {
            items: vec![codex_protocol::user_input::UserInput::Text {
                text: "hello".to_string(),
            }],
        })
        .await
        .expect("Failed to submit prompt");
    wait_for_logged_request(
        &wire_log_dir,
        "session/prompt",
        std::time::Duration::from_secs(5),
    )
    .await
    .expect("session should accept prompts");

    assert_eq!(
        try_latest_logged_request_params(&wire_log_dir, "session/set_config_option"),
        None,
        "value not advertised by the model config option must not be applied"
    );
    assert_eq!(
        try_latest_logged_request_params(&wire_log_dir, "session/set_model"),
        None,
        "the unstable model API must not be used when a model config option exists"
    );

    backend
        .submit(Op::Shutdown)
        .await
        .expect("Failed to shut down ACP backend");
}

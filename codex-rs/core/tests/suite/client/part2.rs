use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn includes_user_instructions_message_in_request() {
    skip_if_no_network!();
    let server = MockServer::start().await;

    let resp_mock = responses::mount_sse_once(&server, sse_completed("resp1")).await;

    let model_provider = ModelProviderInfo {
        base_url: Some(format!("{}/v1", server.uri())),
        ..built_in_model_providers()["openai"].clone()
    };

    let codex_home = TempDir::new().unwrap();
    let mut config = load_default_config_for_test(&codex_home);
    config.model_provider = model_provider;
    config.user_instructions = Some("be nice".to_string());

    let conversation_manager =
        ConversationManager::with_auth(CodexAuth::from_api_key("Test API Key"));
    let codex = conversation_manager
        .new_conversation(config)
        .await
        .expect("create new conversation")
        .conversation;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello".into(),
            }],
        })
        .await
        .unwrap();

    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TaskComplete(_))).await;

    let request = resp_mock.single_request();
    let request_body = request.body_json();

    assert!(
        !request_body["instructions"]
            .as_str()
            .unwrap()
            .contains("be nice")
    );
    assert_message_role(&request_body["input"][0], "user");
    assert_message_starts_with(&request_body["input"][0], "# AGENTS.md instructions for ");
    assert_message_ends_with(&request_body["input"][0], "</INSTRUCTIONS>");
    let ui_text = request_body["input"][0]["content"][0]["text"]
        .as_str()
        .expect("invalid message content");
    assert!(ui_text.contains("<INSTRUCTIONS>"));
    assert!(ui_text.contains("be nice"));
    assert_message_role(&request_body["input"][1], "user");
    assert_message_starts_with(&request_body["input"][1], "<environment_context>");
    assert_message_ends_with(&request_body["input"][1], "</environment_context>");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn includes_configured_effort_in_request() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    let server = MockServer::start().await;

    let resp_mock = responses::mount_sse_once(&server, sse_completed("resp1")).await;
    let TestCodex { codex, .. } = test_codex()
        .with_model("gpt-5.1-codex")
        .with_config(|config| {
            config.model_reasoning_effort = Some(ReasoningEffort::Medium);
        })
        .build(&server)
        .await?;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello".into(),
            }],
        })
        .await
        .unwrap();

    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TaskComplete(_))).await;

    let request = resp_mock.single_request();
    let request_body = request.body_json();

    assert_eq!(
        request_body
            .get("reasoning")
            .and_then(|t| t.get("effort"))
            .and_then(|v| v.as_str()),
        Some("medium")
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn includes_no_effort_in_request() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    let server = MockServer::start().await;

    let resp_mock = responses::mount_sse_once(&server, sse_completed("resp1")).await;
    let TestCodex { codex, .. } = test_codex()
        .with_model("gpt-5.1-codex")
        .build(&server)
        .await?;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello".into(),
            }],
        })
        .await
        .unwrap();

    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TaskComplete(_))).await;

    let request = resp_mock.single_request();
    let request_body = request.body_json();

    assert_eq!(
        request_body
            .get("reasoning")
            .and_then(|t| t.get("effort"))
            .and_then(|v| v.as_str()),
        None
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn includes_default_reasoning_effort_in_request_when_defined_by_model_family()
-> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    let server = MockServer::start().await;

    let resp_mock = responses::mount_sse_once(&server, sse_completed("resp1")).await;
    let TestCodex { codex, .. } = test_codex().with_model("gpt-5.1").build(&server).await?;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello".into(),
            }],
        })
        .await
        .unwrap();

    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TaskComplete(_))).await;

    let request = resp_mock.single_request();
    let request_body = request.body_json();

    assert_eq!(
        request_body
            .get("reasoning")
            .and_then(|t| t.get("effort"))
            .and_then(|v| v.as_str()),
        Some("medium")
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn includes_default_verbosity_in_request() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    let server = MockServer::start().await;

    let resp_mock = responses::mount_sse_once(&server, sse_completed("resp1")).await;
    let TestCodex { codex, .. } = test_codex().with_model("gpt-5.1").build(&server).await?;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello".into(),
            }],
        })
        .await
        .unwrap();

    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TaskComplete(_))).await;

    let request = resp_mock.single_request();
    let request_body = request.body_json();

    assert_eq!(
        request_body
            .get("text")
            .and_then(|t| t.get("verbosity"))
            .and_then(|v| v.as_str()),
        Some("low")
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn configured_verbosity_not_sent_for_models_without_support() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    let server = MockServer::start().await;

    let resp_mock = responses::mount_sse_once(&server, sse_completed("resp1")).await;
    let TestCodex { codex, .. } = test_codex()
        .with_model("gpt-5.1-codex")
        .with_config(|config| {
            config.model_verbosity = Some(Verbosity::High);
        })
        .build(&server)
        .await?;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello".into(),
            }],
        })
        .await
        .unwrap();

    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TaskComplete(_))).await;

    let request = resp_mock.single_request();
    let request_body = request.body_json();

    assert!(
        request_body
            .get("text")
            .and_then(|t| t.get("verbosity"))
            .is_none()
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn configured_verbosity_is_sent() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    let server = MockServer::start().await;

    let resp_mock = responses::mount_sse_once(&server, sse_completed("resp1")).await;
    let TestCodex { codex, .. } = test_codex()
        .with_model("gpt-5.1")
        .with_config(|config| {
            config.model_verbosity = Some(Verbosity::High);
        })
        .build(&server)
        .await?;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello".into(),
            }],
        })
        .await
        .unwrap();

    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TaskComplete(_))).await;

    let request = resp_mock.single_request();
    let request_body = request.body_json();

    assert_eq!(
        request_body
            .get("text")
            .and_then(|t| t.get("verbosity"))
            .and_then(|v| v.as_str()),
        Some("high")
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn includes_developer_instructions_message_in_request() {
    skip_if_no_network!();
    let server = MockServer::start().await;

    let resp_mock = responses::mount_sse_once(&server, sse_completed("resp1")).await;

    let model_provider = ModelProviderInfo {
        base_url: Some(format!("{}/v1", server.uri())),
        ..built_in_model_providers()["openai"].clone()
    };

    let codex_home = TempDir::new().unwrap();
    let mut config = load_default_config_for_test(&codex_home);
    config.model_provider = model_provider;
    config.user_instructions = Some("be nice".to_string());
    config.developer_instructions = Some("be useful".to_string());

    let conversation_manager =
        ConversationManager::with_auth(CodexAuth::from_api_key("Test API Key"));
    let codex = conversation_manager
        .new_conversation(config)
        .await
        .expect("create new conversation")
        .conversation;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello".into(),
            }],
        })
        .await
        .unwrap();

    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TaskComplete(_))).await;

    let request = resp_mock.single_request();
    let request_body = request.body_json();

    assert!(
        !request_body["instructions"]
            .as_str()
            .unwrap()
            .contains("be nice")
    );
    assert_message_role(&request_body["input"][0], "developer");
    assert_message_equals(&request_body["input"][0], "be useful");
    assert_message_role(&request_body["input"][1], "user");
    assert_message_starts_with(&request_body["input"][1], "# AGENTS.md instructions for ");
    assert_message_ends_with(&request_body["input"][1], "</INSTRUCTIONS>");
    let ui_text = request_body["input"][1]["content"][0]["text"]
        .as_str()
        .expect("invalid message content");
    assert!(ui_text.contains("<INSTRUCTIONS>"));
    assert!(ui_text.contains("be nice"));
    assert_message_role(&request_body["input"][2], "user");
    assert_message_starts_with(&request_body["input"][2], "<environment_context>");
    assert_message_ends_with(&request_body["input"][2], "</environment_context>");
}

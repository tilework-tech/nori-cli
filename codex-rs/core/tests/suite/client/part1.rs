use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resume_includes_initial_messages_and_sends_prior_items() {
    skip_if_no_network!();

    // Create a fake rollout session file with prior user + system + assistant messages.
    let tmpdir = TempDir::new().unwrap();
    let session_path = tmpdir.path().join("resume-session.jsonl");
    let mut f = std::fs::File::create(&session_path).unwrap();
    let convo_id = Uuid::new_v4();
    writeln!(
        f,
        "{}",
        json!({
            "timestamp": "2024-01-01T00:00:00.000Z",
            "type": "session_meta",
            "payload": {
                "id": convo_id,
                "timestamp": "2024-01-01T00:00:00Z",
                "instructions": "be nice",
                "cwd": ".",
                "originator": "test_originator",
                "cli_version": "test_version",
                "model_provider": "test-provider"
            }
        })
    )
    .unwrap();

    // Prior item: user message (should be delivered)
    let prior_user = codex_protocol::models::ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![codex_protocol::models::ContentItem::InputText {
            text: "resumed user message".to_string(),
        }],
    };
    let prior_user_json = serde_json::to_value(&prior_user).unwrap();
    writeln!(
        f,
        "{}",
        json!({
            "timestamp": "2024-01-01T00:00:01.000Z",
            "type": "response_item",
            "payload": prior_user_json
        })
    )
    .unwrap();

    // Prior item: system message (excluded from API history)
    let prior_system = codex_protocol::models::ResponseItem::Message {
        id: None,
        role: "system".to_string(),
        content: vec![codex_protocol::models::ContentItem::OutputText {
            text: "resumed system instruction".to_string(),
        }],
    };
    let prior_system_json = serde_json::to_value(&prior_system).unwrap();
    writeln!(
        f,
        "{}",
        json!({
            "timestamp": "2024-01-01T00:00:02.000Z",
            "type": "response_item",
            "payload": prior_system_json
        })
    )
    .unwrap();

    // Prior item: assistant message
    let prior_item = codex_protocol::models::ResponseItem::Message {
        id: None,
        role: "assistant".to_string(),
        content: vec![codex_protocol::models::ContentItem::OutputText {
            text: "resumed assistant message".to_string(),
        }],
    };
    let prior_item_json = serde_json::to_value(&prior_item).unwrap();
    writeln!(
        f,
        "{}",
        json!({
            "timestamp": "2024-01-01T00:00:03.000Z",
            "type": "response_item",
            "payload": prior_item_json
        })
    )
    .unwrap();
    drop(f);

    // Mock server that will receive the resumed request
    let server = MockServer::start().await;
    let resp_mock = responses::mount_sse_once(&server, sse_completed("resp1")).await;

    // Configure Codex to resume from our file
    let model_provider = ModelProviderInfo {
        base_url: Some(format!("{}/v1", server.uri())),
        ..built_in_model_providers()["openai"].clone()
    };
    let codex_home = TempDir::new().unwrap();
    let mut config = load_default_config_for_test(&codex_home);
    config.model_provider = model_provider;
    // Also configure user instructions to ensure they are NOT delivered on resume.
    config.user_instructions = Some("be nice".to_string());

    let conversation_manager =
        ConversationManager::with_auth(CodexAuth::from_api_key("Test API Key"));
    let auth_manager =
        codex_core::AuthManager::from_auth_for_testing(CodexAuth::from_api_key("Test API Key"));
    let NewConversation {
        conversation: codex,
        session_configured,
        ..
    } = conversation_manager
        .resume_conversation_from_rollout(config, session_path.clone(), auth_manager)
        .await
        .expect("resume conversation");

    // 1) Assert initial_messages only includes existing EventMsg entries; response items are not converted
    let initial_msgs = session_configured
        .initial_messages
        .clone()
        .expect("expected initial messages option for resumed session");
    let initial_json = serde_json::to_value(&initial_msgs).unwrap();
    let expected_initial_json = json!([]);
    assert_eq!(initial_json, expected_initial_json);

    // 2) Submit new input; the request body must include the prior item followed by the new user input.
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
    let expected_input = json!([
        {
            "type": "message",
            "role": "user",
            "content": [{ "type": "input_text", "text": "resumed user message" }]
        },
        {
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": "resumed assistant message" }]
        },
        {
            "type": "message",
            "role": "user",
            "content": [{ "type": "input_text", "text": "hello" }]
        }
    ]);
    assert_eq!(request_body["input"], expected_input);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn includes_conversation_id_and_model_headers_in_request() {
    skip_if_no_network!();

    // Mock server
    let server = MockServer::start().await;

    // First request – must NOT include `previous_response_id`.
    let first = ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .set_body_raw(sse_completed("resp1"), "text/event-stream");

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(first)
        .expect(1)
        .mount(&server)
        .await;

    let model_provider = ModelProviderInfo {
        base_url: Some(format!("{}/v1", server.uri())),
        ..built_in_model_providers()["openai"].clone()
    };

    // Init session
    let codex_home = TempDir::new().unwrap();
    let mut config = load_default_config_for_test(&codex_home);
    config.model_provider = model_provider;

    let conversation_manager =
        ConversationManager::with_auth(CodexAuth::from_api_key("Test API Key"));
    let NewConversation {
        conversation: codex,
        conversation_id,
        session_configured: _,
    } = conversation_manager
        .new_conversation(config)
        .await
        .expect("create new conversation");

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello".into(),
            }],
        })
        .await
        .unwrap();

    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TaskComplete(_))).await;

    // get request from the server
    let request = &server.received_requests().await.unwrap()[0];
    let request_conversation_id = request.headers.get("conversation_id").unwrap();
    let request_authorization = request.headers.get("authorization").unwrap();
    let request_originator = request.headers.get("originator").unwrap();

    assert_eq!(
        request_conversation_id.to_str().unwrap(),
        conversation_id.to_string()
    );
    assert_eq!(request_originator.to_str().unwrap(), "nori_cli_rs");
    assert_eq!(
        request_authorization.to_str().unwrap(),
        "Bearer Test API Key"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn includes_base_instructions_override_in_request() {
    skip_if_no_network!();
    // Mock server
    let server = MockServer::start().await;
    let resp_mock = responses::mount_sse_once(&server, sse_completed("resp1")).await;

    let model_provider = ModelProviderInfo {
        base_url: Some(format!("{}/v1", server.uri())),
        ..built_in_model_providers()["openai"].clone()
    };
    let codex_home = TempDir::new().unwrap();
    let mut config = load_default_config_for_test(&codex_home);

    config.base_instructions = Some("test instructions".to_string());
    config.model_provider = model_provider;

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
        request_body["instructions"]
            .as_str()
            .unwrap()
            .contains("test instructions")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chatgpt_auth_sends_correct_request() {
    skip_if_no_network!();

    // Mock server
    let server = MockServer::start().await;

    // First request – must NOT include `previous_response_id`.
    let first = ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .set_body_raw(sse_completed("resp1"), "text/event-stream");

    Mock::given(method("POST"))
        .and(path("/api/codex/responses"))
        .respond_with(first)
        .expect(1)
        .mount(&server)
        .await;

    let model_provider = ModelProviderInfo {
        base_url: Some(format!("{}/api/codex", server.uri())),
        ..built_in_model_providers()["openai"].clone()
    };

    // Init session
    let codex_home = TempDir::new().unwrap();
    let mut config = load_default_config_for_test(&codex_home);
    config.model_provider = model_provider;
    let conversation_manager = ConversationManager::with_auth(create_dummy_codex_auth());
    let NewConversation {
        conversation: codex,
        conversation_id,
        session_configured: _,
    } = conversation_manager
        .new_conversation(config)
        .await
        .expect("create new conversation");

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello".into(),
            }],
        })
        .await
        .unwrap();

    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TaskComplete(_))).await;

    // get request from the server
    let request = &server.received_requests().await.unwrap()[0];
    let request_conversation_id = request.headers.get("conversation_id").unwrap();
    let request_authorization = request.headers.get("authorization").unwrap();
    let request_originator = request.headers.get("originator").unwrap();
    let request_chatgpt_account_id = request.headers.get("chatgpt-account-id").unwrap();
    let request_body = request.body_json::<serde_json::Value>().unwrap();

    assert_eq!(
        request_conversation_id.to_str().unwrap(),
        conversation_id.to_string()
    );
    assert_eq!(request_originator.to_str().unwrap(), "nori_cli_rs");
    assert_eq!(
        request_authorization.to_str().unwrap(),
        "Bearer Access Token"
    );
    assert_eq!(request_chatgpt_account_id.to_str().unwrap(), "account_id");
    assert!(request_body["stream"].as_bool().unwrap());
    assert_eq!(
        request_body["include"][0].as_str().unwrap(),
        "reasoning.encrypted_content"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn prefers_apikey_when_config_prefers_apikey_even_with_chatgpt_tokens() {
    skip_if_no_network!();

    // Mock server
    let server = MockServer::start().await;

    let first = ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .set_body_raw(sse_completed("resp1"), "text/event-stream");

    // Expect API key header, no ChatGPT account header required.
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(header_regex("Authorization", r"Bearer sk-test-key"))
        .respond_with(first)
        .expect(1)
        .mount(&server)
        .await;

    let model_provider = ModelProviderInfo {
        base_url: Some(format!("{}/v1", server.uri())),
        ..built_in_model_providers()["openai"].clone()
    };

    // Init session
    let codex_home = TempDir::new().unwrap();
    // Write auth.json that contains both API key and ChatGPT tokens for a plan that should prefer ChatGPT,
    // but config will force API key preference.
    let _jwt = write_auth_json(
        &codex_home,
        Some("sk-test-key"),
        "pro",
        "Access-123",
        Some("acc-123"),
    );

    let mut config = load_default_config_for_test(&codex_home);
    config.model_provider = model_provider;

    let auth_manager =
        match CodexAuth::from_auth_storage(codex_home.path(), AuthCredentialsStoreMode::File) {
            Ok(Some(auth)) => codex_core::AuthManager::from_auth_for_testing(auth),
            Ok(None) => panic!("No CodexAuth found in codex_home"),
            Err(e) => panic!("Failed to load CodexAuth: {e}"),
        };
    let conversation_manager = ConversationManager::new(auth_manager, SessionSource::Exec);
    let NewConversation {
        conversation: codex,
        ..
    } = conversation_manager
        .new_conversation(config)
        .await
        .expect("create new conversation");

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello".into(),
            }],
        })
        .await
        .unwrap();

    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TaskComplete(_))).await;
}

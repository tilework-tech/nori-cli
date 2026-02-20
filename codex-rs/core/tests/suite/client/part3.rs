use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn azure_responses_request_includes_store_and_reasoning_ids() {
    skip_if_no_network!();

    let server = MockServer::start().await;

    let sse_body = concat!(
        "data: {\"type\":\"response.created\",\"response\":{}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\"}}\n\n",
    );

    let template = ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .set_body_raw(sse_body, "text/event-stream");

    Mock::given(method("POST"))
        .and(path("/openai/responses"))
        .respond_with(template)
        .expect(1)
        .mount(&server)
        .await;

    let provider = ModelProviderInfo {
        name: "azure".into(),
        base_url: Some(format!("{}/openai", server.uri())),
        env_key: None,
        env_key_instructions: None,
        experimental_bearer_token: None,
        wire_api: WireApi::Responses,
        query_params: None,
        http_headers: None,
        env_http_headers: None,
        request_max_retries: Some(0),
        stream_max_retries: Some(0),
        stream_idle_timeout_ms: Some(5_000),
        requires_openai_auth: false,
    };

    let codex_home = TempDir::new().unwrap();
    let mut config = load_default_config_for_test(&codex_home);
    config.model_provider_id = provider.name.clone();
    config.model_provider = provider.clone();
    let effort = config.model_reasoning_effort;
    let summary = config.model_reasoning_summary;
    let config = Arc::new(config);

    let conversation_id = ConversationId::new();

    let otel_event_manager = OtelEventManager::new(
        conversation_id,
        config.model.as_str(),
        config.model_family.slug.as_str(),
        None,
        Some("test@test.com".to_string()),
        Some(AuthMode::ChatGPT),
        false,
        "test".to_string(),
    );

    let client = ModelClient::new(
        Arc::clone(&config),
        None,
        otel_event_manager,
        provider,
        effort,
        summary,
        conversation_id,
        codex_protocol::protocol::SessionSource::Exec,
    );

    let mut prompt = Prompt::default();
    prompt.input.push(ResponseItem::Reasoning {
        id: "reasoning-id".into(),
        summary: vec![ReasoningItemReasoningSummary::SummaryText {
            text: "summary".into(),
        }],
        content: Some(vec![ReasoningItemContent::ReasoningText {
            text: "content".into(),
        }]),
        encrypted_content: None,
    });
    prompt.input.push(ResponseItem::Message {
        id: Some("message-id".into()),
        role: "assistant".into(),
        content: vec![ContentItem::OutputText {
            text: "message".into(),
        }],
    });
    prompt.input.push(ResponseItem::WebSearchCall {
        id: Some("web-search-id".into()),
        status: Some("completed".into()),
        action: WebSearchAction::Search {
            query: Some("weather".into()),
        },
    });
    prompt.input.push(ResponseItem::FunctionCall {
        id: Some("function-id".into()),
        name: "do_thing".into(),
        arguments: "{}".into(),
        call_id: "function-call-id".into(),
    });
    prompt.input.push(ResponseItem::LocalShellCall {
        id: Some("local-shell-id".into()),
        call_id: Some("local-shell-call-id".into()),
        status: LocalShellStatus::Completed,
        action: LocalShellAction::Exec(LocalShellExecAction {
            command: vec!["echo".into(), "hello".into()],
            timeout_ms: None,
            working_directory: None,
            env: None,
            user: None,
        }),
    });
    prompt.input.push(ResponseItem::CustomToolCall {
        id: Some("custom-tool-id".into()),
        status: Some("completed".into()),
        call_id: "custom-tool-call-id".into(),
        name: "custom_tool".into(),
        input: "{}".into(),
    });

    let mut stream = client
        .stream(&prompt)
        .await
        .expect("responses stream to start");

    while let Some(event) = stream.next().await {
        if let Ok(ResponseEvent::Completed { .. }) = event {
            break;
        }
    }

    let requests = server
        .received_requests()
        .await
        .expect("mock server collected requests");
    assert_eq!(requests.len(), 1, "expected a single request");
    let body: serde_json::Value = requests[0]
        .body_json()
        .expect("request body to be valid JSON");

    assert_eq!(body["store"], serde_json::Value::Bool(true));
    assert_eq!(body["stream"], serde_json::Value::Bool(true));
    assert_eq!(body["input"].as_array().map(Vec::len), Some(6));
    assert_eq!(body["input"][0]["id"].as_str(), Some("reasoning-id"));
    assert_eq!(body["input"][1]["id"].as_str(), Some("message-id"));
    assert_eq!(body["input"][2]["id"].as_str(), Some("web-search-id"));
    assert_eq!(body["input"][3]["id"].as_str(), Some("function-id"));
    assert_eq!(body["input"][4]["id"].as_str(), Some("local-shell-id"));
    assert_eq!(body["input"][5]["id"].as_str(), Some("custom-tool-id"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn token_count_includes_rate_limits_snapshot() {
    skip_if_no_network!();
    let server = MockServer::start().await;

    let sse_body = responses::sse(vec![responses::ev_completed_with_tokens("resp_rate", 123)]);

    let response = ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .insert_header("x-codex-primary-used-percent", "12.5")
        .insert_header("x-codex-secondary-used-percent", "40.0")
        .insert_header("x-codex-primary-window-minutes", "10")
        .insert_header("x-codex-secondary-window-minutes", "60")
        .insert_header("x-codex-primary-reset-at", "1704069000")
        .insert_header("x-codex-secondary-reset-at", "1704074400")
        .set_body_raw(sse_body, "text/event-stream");

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(response)
        .expect(1)
        .mount(&server)
        .await;

    let mut provider = built_in_model_providers()["openai"].clone();
    provider.base_url = Some(format!("{}/v1", server.uri()));

    let home = TempDir::new().unwrap();
    let mut config = load_default_config_for_test(&home);
    config.model_provider = provider;

    let conversation_manager = ConversationManager::with_auth(CodexAuth::from_api_key("test"));
    let codex = conversation_manager
        .new_conversation(config)
        .await
        .expect("create conversation")
        .conversation;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello".into(),
            }],
        })
        .await
        .unwrap();

    let first_token_event =
        wait_for_event(&codex, |msg| matches!(msg, EventMsg::TokenCount(_))).await;
    let rate_limit_only = match first_token_event {
        EventMsg::TokenCount(ev) => ev,
        _ => unreachable!(),
    };

    let rate_limit_json = serde_json::to_value(&rate_limit_only).unwrap();
    pretty_assertions::assert_eq!(
        rate_limit_json,
        json!({
            "info": null,
            "rate_limits": {
                "primary": {
                    "used_percent": 12.5,
                    "window_minutes": 10,
                    "resets_at": 1704069000
                },
                "secondary": {
                    "used_percent": 40.0,
                    "window_minutes": 60,
                    "resets_at": 1704074400
                },
                "credits": null
            }
        })
    );

    let token_event = wait_for_event(
        &codex,
        |msg| matches!(msg, EventMsg::TokenCount(ev) if ev.info.is_some()),
    )
    .await;
    let final_payload = match token_event {
        EventMsg::TokenCount(ev) => ev,
        _ => unreachable!(),
    };
    // Assert full JSON for the final token count event (usage + rate limits)
    let final_json = serde_json::to_value(&final_payload).unwrap();
    pretty_assertions::assert_eq!(
        final_json,
        json!({
            "info": {
                "total_token_usage": {
                    "input_tokens": 123,
                    "cached_input_tokens": 0,
                    "output_tokens": 0,
                    "reasoning_output_tokens": 0,
                    "total_tokens": 123
                },
                "last_token_usage": {
                    "input_tokens": 123,
                    "cached_input_tokens": 0,
                    "output_tokens": 0,
                    "reasoning_output_tokens": 0,
                    "total_tokens": 123
                },
                // Default model is gpt-5.1-codex-max in tests → 95% usable context window
                "model_context_window": 258400
            },
            "rate_limits": {
                "primary": {
                    "used_percent": 12.5,
                    "window_minutes": 10,
                    "resets_at": 1704069000
                },
                "secondary": {
                    "used_percent": 40.0,
                    "window_minutes": 60,
                    "resets_at": 1704074400
                },
                "credits": null
            }
        })
    );
    let usage = final_payload
        .info
        .expect("token usage info should be recorded after completion");
    assert_eq!(usage.total_token_usage.total_tokens, 123);
    let final_snapshot = final_payload
        .rate_limits
        .expect("latest rate limit snapshot should be retained");
    assert_eq!(
        final_snapshot
            .primary
            .as_ref()
            .map(|window| window.used_percent),
        Some(12.5)
    );
    assert_eq!(
        final_snapshot
            .primary
            .as_ref()
            .and_then(|window| window.resets_at),
        Some(1704069000)
    );

    wait_for_event(&codex, |msg| matches!(msg, EventMsg::TaskComplete(_))).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn usage_limit_error_emits_rate_limit_event() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    let server = MockServer::start().await;

    let response = ResponseTemplate::new(429)
        .insert_header("x-codex-primary-used-percent", "100.0")
        .insert_header("x-codex-secondary-used-percent", "87.5")
        .insert_header("x-codex-primary-over-secondary-limit-percent", "95.0")
        .insert_header("x-codex-primary-window-minutes", "15")
        .insert_header("x-codex-secondary-window-minutes", "60")
        .set_body_json(json!({
            "error": {
                "type": "usage_limit_reached",
                "message": "limit reached",
                "resets_at": 1704067242,
                "plan_type": "pro"
            }
        }));

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(response)
        .expect(1)
        .mount(&server)
        .await;

    let mut builder = test_codex();
    let codex_fixture = builder.build(&server).await?;
    let codex = codex_fixture.codex.clone();

    let expected_limits = json!({
        "primary": {
            "used_percent": 100.0,
            "window_minutes": 15,
            "resets_at": null
        },
        "secondary": {
            "used_percent": 87.5,
            "window_minutes": 60,
            "resets_at": null
        },
        "credits": null
    });

    let submission_id = codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello".into(),
            }],
        })
        .await
        .expect("submission should succeed while emitting usage limit error events");

    let token_event = wait_for_event(&codex, |msg| matches!(msg, EventMsg::TokenCount(_))).await;
    let EventMsg::TokenCount(event) = token_event else {
        unreachable!();
    };

    let event_json = serde_json::to_value(&event).expect("serialize token count event");
    pretty_assertions::assert_eq!(
        event_json,
        json!({
            "info": null,
            "rate_limits": expected_limits
        })
    );

    let error_event = wait_for_event(&codex, |msg| matches!(msg, EventMsg::Error(_))).await;
    let EventMsg::Error(error_event) = error_event else {
        unreachable!();
    };
    assert!(
        error_event.message.to_lowercase().contains("usage limit"),
        "unexpected error message for submission {submission_id}: {}",
        error_event.message
    );

    Ok(())
}

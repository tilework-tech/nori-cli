use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_compact_triggers_after_function_call_over_95_percent_usage() {
    skip_if_no_network!();

    let server = start_mock_server().await;

    let context_window = 100;
    let limit = context_window * 90 / 100;
    let over_limit_tokens = context_window * 95 / 100 + 1;

    let first_turn = sse(vec![
        ev_function_call(DUMMY_CALL_ID, DUMMY_FUNCTION_NAME, "{}"),
        ev_completed_with_tokens("r1", 50),
    ]);
    let function_call_follow_up = sse(vec![
        ev_assistant_message("m2", FINAL_REPLY),
        ev_completed_with_tokens("r2", over_limit_tokens),
    ]);
    let auto_summary_payload = auto_summary(AUTO_SUMMARY_TEXT);
    let auto_compact_turn = sse(vec![
        ev_assistant_message("m3", &auto_summary_payload),
        ev_completed_with_tokens("r3", 10),
    ]);
    let post_auto_compact_turn = sse(vec![ev_completed_with_tokens("r4", 10)]);

    // Mount responses in order and keep mocks only for the ones we assert on.
    let first_turn_mock = mount_sse_once(&server, first_turn).await;
    let follow_up_mock = mount_sse_once(&server, function_call_follow_up).await;
    let auto_compact_mock = mount_sse_once(&server, auto_compact_turn).await;
    // We don't assert on the post-compact request, so no need to keep its mock.
    mount_sse_once(&server, post_auto_compact_turn).await;

    let model_provider = ModelProviderInfo {
        base_url: Some(format!("{}/v1", server.uri())),
        ..built_in_model_providers()["openai"].clone()
    };

    let home = TempDir::new().unwrap();
    let mut config = load_default_config_for_test(&home);
    config.model_provider = model_provider;
    set_test_compact_prompt(&mut config);
    config.model_context_window = Some(context_window);
    config.model_auto_compact_token_limit = Some(limit);

    let codex = ConversationManager::with_auth(CodexAuth::from_api_key("dummy"))
        .new_conversation(config)
        .await
        .unwrap()
        .conversation;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: FUNCTION_CALL_LIMIT_MSG.into(),
            }],
        })
        .await
        .unwrap();

    wait_for_event(&codex, |msg| matches!(msg, EventMsg::TaskComplete(_))).await;

    // Assert first request captured expected user message that triggers function call.
    let first_request = first_turn_mock.single_request().input();
    assert!(
        first_request.iter().any(|item| {
            item.get("type").and_then(|value| value.as_str()) == Some("message")
                && item
                    .get("content")
                    .and_then(|content| content.as_array())
                    .and_then(|entries| entries.first())
                    .and_then(|entry| entry.get("text"))
                    .and_then(|value| value.as_str())
                    == Some(FUNCTION_CALL_LIMIT_MSG)
        }),
        "first request should include the user message that triggers the function call"
    );

    let function_call_output = follow_up_mock
        .single_request()
        .function_call_output(DUMMY_CALL_ID);
    let output_text = function_call_output
        .get("output")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    assert!(
        output_text.contains(DUMMY_FUNCTION_NAME),
        "function call output should be sent before auto compact"
    );

    let auto_compact_body = auto_compact_mock.single_request().body_json().to_string();
    assert!(
        body_contains_text(&auto_compact_body, SUMMARIZATION_PROMPT),
        "auto compact request should include the summarization prompt after exceeding 95% (limit {limit})"
    );
}

use agent_client_protocol::Client;
use agent_client_protocol::ConnectTo;
use agent_client_protocol::RawJsonRpcMessage;
use futures::StreamExt;
use nori_protocol::acp::v1 as acp;
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio::time::Duration;

#[tokio::test]
async fn cancel_before_prompt_work_starts_is_retained_and_next_turn_can_stream() {
    let (mut channel, serve) = ConnectTo::<Client>::into_channel_and_future(super::mock_agent());
    let prompt = json!({
        "sessionId": "cancellation-test",
        "prompt": [{ "type": "text", "text": "mock:stream-until-cancel" }],
    });
    let cancel = json!({ "sessionId": "cancellation-test" });

    // Queue both messages before polling the connection. The SDK dispatches
    // ready incoming messages in order before polling work queued by cx.spawn.
    channel
        .tx
        .unbounded_send(Ok(RawJsonRpcMessage::request(
            "session/prompt".into(),
            prompt.clone(),
            acp::RequestId::Number(1),
        )
        .unwrap()))
        .unwrap();
    channel
        .tx
        .unbounded_send(Ok(RawJsonRpcMessage::notification(
            "session/cancel".into(),
            cancel.clone(),
        )
        .unwrap()))
        .unwrap();

    let agent = tokio::spawn(serve);
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        let mut turn = 1;
        let mut second_turn_streamed = false;
        while let Some(message) = channel.rx.next().await {
            let message = message.unwrap();
            match message {
                RawJsonRpcMessage::Notification(notification) => {
                    let notification = serde_json::to_value(notification).unwrap();
                    if notification["params"]["update"]["content"]["text"] == "Streaming..." {
                        assert_eq!(turn, 2, "first turn lost its pre-dispatched cancel");
                        if !second_turn_streamed {
                            second_turn_streamed = true;
                            channel
                                .tx
                                .unbounded_send(Ok(RawJsonRpcMessage::notification(
                                    "session/cancel".into(),
                                    cancel.clone(),
                                )
                                .unwrap()))
                                .unwrap();
                        }
                    }
                }
                RawJsonRpcMessage::Response(acp::Response::Result { id, result }) => {
                    assert_eq!(id, acp::RequestId::Number(turn));
                    let response: acp::PromptResponse = serde_json::from_value(result).unwrap();
                    assert_eq!(response.stop_reason, acp::StopReason::Cancelled);
                    if turn == 2 {
                        assert!(second_turn_streamed, "cancel leaked into the next turn");
                        return;
                    }
                    turn = 2;
                    channel
                        .tx
                        .unbounded_send(Ok(RawJsonRpcMessage::request(
                            "session/prompt".into(),
                            prompt.clone(),
                            acp::RequestId::Number(turn),
                        )
                        .unwrap()))
                        .unwrap();
                }
                RawJsonRpcMessage::Response(acp::Response::Error { error, .. }) => {
                    panic!("prompt failed: {error:?}");
                }
                RawJsonRpcMessage::Request(request) => {
                    panic!("unexpected agent request: {request:?}");
                }
            }
        }
        panic!("agent closed before both turns completed");
    })
    .await;
    agent.abort();
    let _ = agent.await;
    result.expect("both cancelled turns should complete");
}

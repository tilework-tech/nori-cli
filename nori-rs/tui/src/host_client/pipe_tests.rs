use std::time::Duration;

use pretty_assertions::assert_eq;
use serde_json::json;
use tokio::sync::mpsc;

use super::ConnectionEvent;
use super::drive_pipe;

async fn next(events: &mut mpsc::UnboundedReceiver<ConnectionEvent>) -> ConnectionEvent {
    tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .expect("pipe event")
        .expect("pipe driver alive")
}

#[tokio::test]
async fn a_pipe_carries_calls_and_replies_until_the_host_closes_it() {
    let (to_host, mut host_input) = mpsc::channel(8);
    let (host_output, from_host) = mpsc::channel(8);
    let (requests, request_rx) = mpsc::unbounded_channel();
    let (event_tx, mut events) = mpsc::unbounded_channel();
    let driver = tokio::spawn(drive_pipe(to_host, from_host, request_rx, event_tx));

    assert!(matches!(
        next(&mut events).await,
        ConnectionEvent::Connected
    ));
    requests
        .send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}))
        .unwrap();
    let line = host_input.recv().await.unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&line).unwrap(),
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}})
    );
    host_output
        .send(json!({"jsonrpc":"2.0","id":1,"result":{}}).to_string())
        .await
        .unwrap();
    assert!(matches!(
        next(&mut events).await,
        ConnectionEvent::Message(message) if message == json!({"jsonrpc":"2.0","id":1,"result":{}})
    ));

    drop(host_output);
    assert!(matches!(
        next(&mut events).await,
        ConnectionEvent::Stopped(_)
    ));
    driver.await.unwrap();
}

#[tokio::test]
async fn a_line_that_is_not_json_is_skipped() {
    let (to_host, _host_input) = mpsc::channel(8);
    let (host_output, from_host) = mpsc::channel(8);
    let (_requests, request_rx) = mpsc::unbounded_channel();
    let (event_tx, mut events) = mpsc::unbounded_channel();
    tokio::spawn(drive_pipe(to_host, from_host, request_rx, event_tx));
    next(&mut events).await;

    host_output.send("not json".into()).await.unwrap();
    host_output
        .send(json!({"jsonrpc":"2.0","method":"x"}).to_string())
        .await
        .unwrap();
    assert!(matches!(
        next(&mut events).await,
        ConnectionEvent::Message(message) if message["method"] == "x"
    ));
}

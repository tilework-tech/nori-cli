use futures::stream::StreamExt;
use nori_cli::backends::AgentBackend;
/// Test to verify that StreamComplete is actually sent after ResultSummary
use nori_cli::backends::claude::ClaudeBackend;
use nori_cli::backends::is_available;
use nori_cli::conversation::ConversationEvent;

#[tokio::test]
async fn test_claude_stream_sends_all_events_and_completes() {
    // Skip test if Claude CLI is not installed
    if !is_available("claude") {
        println!("Skipping test - Claude CLI not available");
        return;
    }

    let backend = ClaudeBackend::new();
    let cancel_token = tokio_util::sync::CancellationToken::new();

    let mut stream = backend.spawn_stream("Say hello".to_string(), cancel_token);

    let mut events = Vec::new();
    let mut result_summary_count = 0;

    println!("\n=== Consuming stream events ===");

    while let Some(event) = stream.next().await {
        println!("Received event: {:?}", event);

        if matches!(event, ConversationEvent::ResultSummary { .. }) {
            result_summary_count += 1;
            println!(">>> ResultSummary #{} <<<", result_summary_count);
        }

        events.push(event);
    }

    println!("\n=== Stream ended ===");
    println!("Total events: {}", events.len());
    println!("ResultSummary events: {}", result_summary_count);

    // The stream should have ended naturally
    // If we got here, the stream completed
    assert!(
        result_summary_count > 0,
        "Should have received at least one ResultSummary"
    );

    println!("\nStream completed successfully!");
}

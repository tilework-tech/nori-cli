/// Diagnostic test to observe actual Claude subprocess behavior
/// Run with: cargo test diagnose_claude_stream -- --nocapture --test-threads=1
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::time::{Duration, timeout};

#[tokio::test]
async fn diagnose_claude_stream() {
    println!("\n=== Starting Claude subprocess diagnostic ===\n");

    // Spawn Claude with a simple prompt - using exact same args as backend
    let mut cmd = Command::new("claude");
    cmd.arg("--print")
        .arg("--output-format")
        .arg("stream-json")
        .arg("--include-partial-messages")
        .arg("--verbose")
        .arg("Say 'hello' and nothing else")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            println!("Failed to spawn claude: {e}");
            println!("Skipping test - Claude CLI not available");
            return;
        }
    };

    println!("Claude subprocess spawned successfully\n");

    // Read stdout line by line with timeout
    if let Some(stdout) = child.stdout.take() {
        let mut reader = BufReader::new(stdout).lines();
        let mut line_count = 0;
        let mut saw_result_summary = false;

        println!("Reading stdout lines:\n");

        loop {
            match timeout(Duration::from_secs(5), reader.next_line()).await {
                Ok(Ok(Some(line))) => {
                    line_count += 1;
                    println!("[Line {line_count}] {line}");

                    // Check if this is a result event
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
                        if json.get("type").and_then(|v| v.as_str()) == Some("result") {
                            saw_result_summary = true;
                            println!(
                                "\n>>> Found ResultSummary event at line {line_count} <<<\n"
                            );
                        }
                    }

                    // If we saw result summary, wait a bit to see if more lines come
                    if saw_result_summary {
                        println!(
                            "Waiting 2 seconds to see if more lines arrive after ResultSummary..."
                        );
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                }
                Ok(Ok(None)) => {
                    println!("\n>>> stdout EOF reached after {line_count} lines <<<");
                    break;
                }
                Ok(Err(e)) => {
                    println!("\n>>> Error reading line: {e} <<<");
                    break;
                }
                Err(_) => {
                    println!(
                        "\n>>> Timeout waiting for next line (after {line_count} lines) <<<"
                    );
                    println!("This suggests stdout is still open but no more data is coming");
                    break;
                }
            }
        }

        println!(
            "\nSaw ResultSummary: {}",
            if saw_result_summary { "YES" } else { "NO" }
        );
    }

    // Check process status
    println!("\nChecking process status...");
    match timeout(Duration::from_secs(2), child.wait()).await {
        Ok(Ok(status)) => {
            println!("Process exited with status: {status}");
        }
        Ok(Err(e)) => {
            println!("Error waiting for process: {e}");
        }
        Err(_) => {
            println!("Process still running after timeout - killing it");
            let _ = child.kill().await;
        }
    }

    println!("\n=== Diagnostic complete ===\n");
}

//! Deterministic context observations for protocol and terminal tests.

use crate::MockAgent;
use crate::acp;
use std::sync::atomic::Ordering;

pub(crate) async fn emit(agent: &MockAgent, session_id: &acp::SessionId) -> Result<(), acp::Error> {
    let updates = if let Ok(sequence) = std::env::var("MOCK_AGENT_CONTEXT_SEQUENCE") {
        let turns: Vec<Vec<acp::UsageUpdate>> =
            serde_json::from_str(&sequence).map_err(|error| {
                acp::Error::new(-32602, format!("invalid context fixture: {error}"))
            })?;
        let index = agent
            .state
            .context_prompt_index
            .fetch_add(1, Ordering::SeqCst);
        turns.into_iter().nth(index as usize).unwrap_or_default()
    } else if std::env::var("MOCK_AGENT_CONTEXT_USAGE").is_ok() {
        vec![acp::UsageUpdate::new(340_412, 1_000_000)]
    } else {
        Vec::new()
    };
    for usage in updates {
        agent
            .send_update(session_id.clone(), acp::SessionUpdate::UsageUpdate(usage))
            .await?;
    }
    if let Ok(release_file) = std::env::var("MOCK_AGENT_CONTEXT_RELEASE_FILE") {
        agent
            .send_text_chunk(session_id.clone(), "Context reading ready.")
            .await?;
        while !std::path::Path::new(&release_file).exists() && !agent.cancel_requested() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
    Ok(())
}

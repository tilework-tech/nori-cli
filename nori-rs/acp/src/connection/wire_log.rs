use std::fs::File;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use anyhow::Context;
use anyhow::Result;
use serde_json::Value;
use serde_json::json;
use tracing::warn;

use crate::config::AcpProxyConfig;
use crate::registry::AcpAgentConfig;

#[derive(Clone)]
pub(super) struct WireLogger {
    inner: Arc<Mutex<File>>,
    agent: String,
    child_pid: i64,
}

impl WireLogger {
    pub(super) fn new(
        config: &AcpProxyConfig,
        agent_config: &AcpAgentConfig,
        pid: u32,
    ) -> Result<Self> {
        std::fs::create_dir_all(&config.log_dir).with_context(|| {
            format!(
                "Failed to create ACP wire log directory {}",
                config.log_dir.display()
            )
        })?;

        let child_pid = i64::from(pid);
        let path = log_path(&config.log_dir, &agent_config.provider_slug, child_pid);
        let file = OpenOptions::new()
            .create_new(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("Failed to open ACP wire log {}", path.display()))?;

        Ok(Self {
            inner: Arc::new(Mutex::new(file)),
            agent: agent_config.provider_slug.clone(),
            child_pid,
        })
    }

    pub(super) fn record(&self, direction: WireDirection, line: &str) {
        if let Err(error) = self.try_record(direction, line) {
            warn!("Failed to write ACP wire log entry: {error}");
        }
    }

    fn try_record(&self, direction: WireDirection, line: &str) -> Result<()> {
        let record = match serde_json::from_str::<Value>(line) {
            Ok(message) => json!({
                "ts_ms": unix_time_millis(),
                "direction": direction.as_str(),
                "agent": self.agent,
                "child_pid": self.child_pid,
                "message": message,
            }),
            Err(error) => json!({
                "ts_ms": unix_time_millis(),
                "direction": direction.as_str(),
                "agent": self.agent,
                "child_pid": self.child_pid,
                "raw_line": line,
                "parse_error": error.to_string(),
            }),
        };

        let mut file = self
            .inner
            .lock()
            .map_err(|error| anyhow::anyhow!("ACP wire log lock poisoned: {error}"))?;
        serde_json::to_writer(&mut *file, &record).context("Failed to serialize ACP wire log")?;
        file.write_all(b"\n")
            .context("Failed to append ACP wire log newline")?;
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub(super) enum WireDirection {
    ClientToAgent,
    AgentToClient,
}

impl WireDirection {
    fn as_str(self) -> &'static str {
        match self {
            Self::ClientToAgent => "client_to_agent",
            Self::AgentToClient => "agent_to_client",
        }
    }
}

fn log_path(log_dir: &Path, agent_slug: &str, child_pid: i64) -> std::path::PathBuf {
    let timestamp = unix_time_millis();
    let agent = sanitize_filename(agent_slug);
    log_dir.join(format!("{timestamp}-{child_pid}-{agent}.jsonl"))
}

fn sanitize_filename(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn unix_time_millis() -> i64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    i64::try_from(millis).unwrap_or(i64::MAX)
}

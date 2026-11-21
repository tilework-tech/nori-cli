//! Public metadata for the authenticated host inventory.

use anyhow::Context;
use anyhow::ensure;
use serde::Deserialize;
use serde::Serialize;

use super::LaunchSpec;
use super::ManagedLaunches;
use crate::ConversationId;

pub(super) const SUBSCRIPTION_LABEL: &str = "sessions:codex-subscription-file:v1";
const TOKEN_PREFIX: &str = "sessions:token-connection:";

fn token_label(reference: &str) -> bool {
    reference
        .strip_prefix(TOKEN_PREFIX)
        .and_then(|rest| rest.strip_suffix(":v1"))
        .is_some_and(|id| {
            let alphanumeric = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
            (1..=64).contains(&id.len())
                && alphanumeric(id.as_bytes()[0])
                && id.bytes().all(|byte| alphanumeric(byte) || byte == b'-')
        })
}

impl LaunchSpec {
    pub(super) fn validate_credential_refs(&self) -> anyhow::Result<()> {
        for reference in &self.credential_refs {
            ensure!(
                !reference.starts_with(TOKEN_PREFIX) || token_label(reference),
                "invalid token-connection reference"
            );
        }
        Ok(())
    }
}

/// The credential-safe projection shared by managed and ordinary host listings.
#[derive(Deserialize, Serialize)]
pub struct HostSessionIdentity {
    session_id: ConversationId,
    #[serde(default)]
    credential_refs: Vec<String>,
}

#[derive(Serialize)]
pub struct ManagedSession {
    #[serde(flatten)]
    identity: HostSessionIdentity,
    agent: String,
    status: &'static str,
}

impl ManagedLaunches {
    pub fn list(&self) -> anyhow::Result<Vec<ManagedSession>> {
        let guard = self
            .launches
            .lock()
            .map_err(|_| anyhow::anyhow!("launch registry poisoned"))?;
        let mut entries: Vec<_> = guard.as_ref().context("host is stopping")?.iter().collect();
        entries.sort_unstable_by(|(a, _), (b, _)| a.cmp(b));
        Ok(entries
            .into_iter()
            .map(|(binding, launch)| ManagedSession {
                identity: HostSessionIdentity {
                    session_id: launch.conversation_id,
                    credential_refs: launch
                        .spec
                        .credential_refs
                        .iter()
                        .filter(|label| label.as_str() == SUBSCRIPTION_LABEL || token_label(label))
                        .cloned()
                        .collect(),
                },
                agent: launch.spec.agent.clone(),
                status: launch.receipt(binding, true).state,
            })
            .collect())
    }
}

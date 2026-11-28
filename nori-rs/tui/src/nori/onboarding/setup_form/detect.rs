//! Which agents Nori can already see a sign-in for.

use std::path::Path;

use nori_auth_providers::AuthStatus;
use nori_config::NoriConfig;
use nori_harness::AgentKind;

use super::AgentChoice;

/// Every built-in agent with its sign-in status, in the registry's order.
/// May block briefly on the OS keyring.
pub(crate) fn detect_agents(nori_home: &Path, config: &NoriConfig) -> Vec<AgentChoice> {
    let home = dirs::home_dir().unwrap_or_default();
    AgentKind::all()
        .iter()
        .map(|kind| {
            let prefer = config.auth_preference(kind.slug()).prefer_api_key;
            let status = nori_auth_providers::detect_local_agent(*kind, &home, nori_home, prefer)
                .map_or(AuthStatus::Unknown, |local| local.status.status);
            AgentChoice {
                kind: *kind,
                status,
            }
        })
        .collect()
}

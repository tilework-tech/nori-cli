//! Persist the setup answers.

use std::path::Path;

use nori_config::NoriConfigEdits;

use super::Permission;
use super::SetupChoices;

/// Save the default agent, the permission mode when it differs from the
/// default (Agent leaves each folder's trust in charge), and the usage
/// opt-out when the user declines.
pub(crate) fn save(nori_home: &Path, choices: &SetupChoices) -> anyhow::Result<()> {
    let mut edits = NoriConfigEdits::new(nori_home).set_path(&["agent"], choices.agent.slug());
    let policy = match choices.permission {
        Permission::Ask => Some("untrusted"),
        Permission::Agent => None,
        Permission::ApproveAll => Some("never"),
    };
    if let Some(policy) = policy {
        edits = edits.set_path(&["approval_policy"], policy);
    }
    edits.apply_blocking()?;
    if !choices.share_usage {
        nori_installed::set_opt_out(nori_home, true)?;
    }
    Ok(())
}

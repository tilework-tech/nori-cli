//! The one-time move from the retired approval presets to per-session modes.
//!
//! `/approvals` used to offer three presets that set `approval_policy` and
//! `sandbox_mode` together. Sessions now take one of the host's modes (Ask,
//! Agent, Approve all) and the sandbox is chosen on its own. A config whose
//! two keys spell out a preset exactly gets the matching
//! `[host] default_mode`; both original keys stay as they were. Any other
//! explicit pair is reported once and left alone. `[notice]
//! approval_presets_migrated` records that the move ran.

use std::path::Path;

use anyhow::Result;
use toml_edit::Item;
use toml_edit::value;

use crate::CONFIG_FILE;
use crate::edits::read_for_edit;
use crate::edits::write_config;

const MARKER: &str = "approval_presets_migrated";

struct Preset {
    name: &'static str,
    approval_policy: &'static str,
    sandbox_mode: &'static str,
    mode_id: &'static str,
    mode_name: &'static str,
}

const PRESETS: [Preset; 3] = [
    Preset {
        name: "Read Only",
        approval_policy: "on-request",
        sandbox_mode: "read-only",
        mode_id: "ask",
        mode_name: "Ask",
    },
    Preset {
        name: "Agent",
        approval_policy: "on-request",
        sandbox_mode: "workspace-write",
        mode_id: "agent",
        mode_name: "Agent",
    },
    Preset {
        name: "Full Access",
        approval_policy: "never",
        sandbox_mode: "danger-full-access",
        mode_id: "approve-all",
        mode_name: "Approve all",
    },
];

/// Run the move once for the config in `nori_home`. Returns the line to
/// show the user when the config changed or was reported; `None` when there
/// was nothing to do.
pub fn migrate_approval_presets(nori_home: &Path) -> Result<Option<String>> {
    if !nori_home.join(CONFIG_FILE).exists() {
        return Ok(None);
    }
    let (_lock, mut document) = read_for_edit(nori_home)?;
    let marked = document
        .get("notice")
        .and_then(|notice| notice.get(MARKER))
        .and_then(Item::as_bool)
        .unwrap_or(false);
    let has_default_mode = document
        .get("host")
        .and_then(|host| host.get("default_mode"))
        .is_some();
    let key = |name: &str| document.get(name).and_then(Item::as_str).map(str::to_owned);
    let (approval_policy, sandbox_mode) = (key("approval_policy"), key("sandbox_mode"));
    if marked || has_default_mode || (approval_policy.is_none() && sandbox_mode.is_none()) {
        return Ok(None);
    }
    let preset = PRESETS.iter().find(|preset| {
        approval_policy.as_deref() == Some(preset.approval_policy)
            && sandbox_mode.as_deref() == Some(preset.sandbox_mode)
    });
    let message = match preset {
        Some(preset) => {
            document["host"]["default_mode"] = value(preset.mode_id);
            format!(
                "Approval preset {} is now `[host] default_mode = \"{}\"`: new sessions start in {}. \
                 sandbox_mode stays \"{}\"; /approvals no longer changes it.",
                preset.name, preset.mode_id, preset.mode_name, preset.sandbox_mode
            )
        }
        None => {
            let shown = |setting: Option<String>| setting.unwrap_or_else(|| "unset".to_owned());
            format!(
                "approval_policy ({}) and sandbox_mode ({}) match none of the retired approval presets, \
                 so they were left as is. /approvals now picks Ask, Agent or Approve all for one session; \
                 set `[host] default_mode` to choose how new sessions start.",
                shown(approval_policy),
                shown(sandbox_mode)
            )
        }
    };
    document["notice"][MARKER] = value(true);
    write_config(
        &nori_home.join(CONFIG_FILE),
        document.to_string().as_bytes(),
    )?;
    Ok(Some(message))
}

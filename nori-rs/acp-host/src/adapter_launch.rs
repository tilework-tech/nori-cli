//! Launch control for ACP adapters: per-agent command overrides, package
//! version pins, and the notice shown before npx/bunx downloads an adapter.
//!
//! Precedence for one agent: `NORI_ACP_COMMAND_<SLUG>` env, then
//! `[acp_adapters.<slug>] command`, then `[acp_adapters.<slug>] version`, then
//! the built-in `@latest` package.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::PoisonError;
use std::sync::RwLock;

use anyhow::Result;
use nori_config::AcpAdapterToml;
use sha2::Digest;
use sha2::Sha512;

use crate::registry::PackageManager;

/// Overrides from `[acp_adapters]`, keyed as written in config.
static ADAPTER_OVERRIDES: RwLock<Option<HashMap<String, AcpAdapterToml>>> = RwLock::new(None);

/// Prefix of the per-agent command override variable.
pub const COMMAND_ENV_PREFIX: &str = "NORI_ACP_COMMAND_";

/// Replace the `[acp_adapters]` launch overrides. Rejects a `command` that does
/// not shell-split into at least one word and a `version` that is not a plain
/// npm version, range, or dist-tag. Keys may be any agent slug or alias.
pub fn set_adapter_launch_overrides(adapters: HashMap<String, AcpAdapterToml>) -> Result<()> {
    for (slug, adapter) in &adapters {
        if let Some(command) = &adapter.command {
            split_command(command)
                .map_err(|reason| anyhow::anyhow!("acp_adapters.{slug}.command {reason}"))?;
        }
        if let Some(version) = &adapter.version {
            let valid = !version.is_empty()
                && version
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || ".-+^~<>=*_".contains(c));
            anyhow::ensure!(
                valid,
                "acp_adapters.{slug}.version must be an npm version, range, or dist-tag without spaces, got {version:?}"
            );
        }
    }
    *ADAPTER_OVERRIDES
        .write()
        .unwrap_or_else(PoisonError::into_inner) = Some(adapters);
    Ok(())
}

/// The environment variable that overrides `slug`'s launch command.
pub fn command_env_var(slug: &str) -> String {
    format!(
        "{COMMAND_ENV_PREFIX}{}",
        slug.to_ascii_uppercase().replace('-', "_")
    )
}

fn split_command(command: &str) -> std::result::Result<(String, Vec<String>), &'static str> {
    let mut words = shlex::split(command)
        .ok_or("has unbalanced quotes")?
        .into_iter();
    let program = words.next().ok_or("is empty")?;
    Ok((program, words.collect()))
}

/// The `[acp_adapters]` entry for canonical `slug`: its exact key first, then
/// the alphabetically first alias key that resolves to it.
fn configured(slug: &str) -> Option<AcpAdapterToml> {
    let guard = ADAPTER_OVERRIDES
        .read()
        .unwrap_or_else(PoisonError::into_inner);
    let adapters = guard.as_ref()?;
    adapters
        .get(slug)
        .or_else(|| {
            adapters
                .iter()
                .filter(|(key, _)| crate::registry::resolve_agent_slug(key) == slug)
                .min_by_key(|(key, _)| key.as_str())
                .map(|(_, adapter)| adapter)
        })
        .cloned()
}

/// The replacement launch command for canonical `slug`, if any.
pub(crate) fn command_override(slug: &str) -> Result<Option<(String, Vec<String>)>> {
    let var = command_env_var(slug);
    if let Some(value) = std::env::var_os(&var) {
        let value = value
            .into_string()
            .map_err(|_| anyhow::anyhow!("{var} is not valid UTF-8"))?;
        return split_command(&value)
            .map(Some)
            .map_err(|reason| anyhow::anyhow!("{var} {reason}"));
    }
    match configured(slug).and_then(|adapter| adapter.command) {
        Some(command) => split_command(&command)
            .map(Some)
            .map_err(|reason| anyhow::anyhow!("acp_adapters.{slug}.command {reason}")),
        None => Ok(None),
    }
}

/// The configured package version for canonical `slug`, if any.
pub(crate) fn version_pin(slug: &str) -> Option<String> {
    configured(slug).and_then(|adapter| adapter.version)
}

/// A "Downloading …" notice when `manager` has not cached the package set
/// `specs` (each `name@version`), else `None`. The first spec is the package
/// whose binary runs.
pub(crate) fn download_notice(manager: PackageManager, specs: &[String]) -> Option<String> {
    let first = specs.first()?;
    let installed = match manager {
        PackageManager::Npm => npx_install_dir(specs),
        PackageManager::Bun => bunx_install_dir(first),
    }
    .map(|dir| {
        dir.join("node_modules")
            .join(package_name(first))
            .join("package.json")
    });
    if installed.is_some_and(|path| path.is_file()) {
        return None;
    }
    Some(format!("Downloading {}…", specs.join(", ")))
}

fn package_name(spec: &str) -> &str {
    // Skip a leading scope `@` before looking for the version separator.
    let scoped = usize::from(spec.starts_with('@'));
    match spec[scoped..].find('@') {
        Some(index) => &spec[..scoped + index],
        None => spec,
    }
}

/// npx installs into `<cache>/_npx/<first 16 hex of sha512(sorted specs joined by "\n")>`
/// (libnpmexec, npm 10). npm sorts with `localeCompare`; byte order agrees for
/// the built-in package names.
fn npx_install_dir(specs: &[String]) -> Option<PathBuf> {
    let cache = std::env::var_os("npm_config_cache")
        .or_else(|| std::env::var_os("NPM_CONFIG_CACHE"))
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".npm")))?;
    let mut sorted = specs.to_vec();
    sorted.sort();
    let digest = Sha512::digest(sorted.join("\n").as_bytes());
    let hash: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
    Some(cache.join("_npx").join(hash))
}

/// bun 1.x installs `bunx <spec>` into `$TMPDIR/bunx-<uid>-<spec>` on Unix.
#[cfg(unix)]
fn bunx_install_dir(spec: &str) -> Option<PathBuf> {
    // SAFETY: getuid has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    Some(std::env::temp_dir().join(format!("bunx-{uid}-{spec}")))
}

#[cfg(not(unix))]
fn bunx_install_dir(_spec: &str) -> Option<PathBuf> {
    None
}

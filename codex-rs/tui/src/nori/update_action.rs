//! Nori-specific update action definitions.
//!
//! This module defines the update actions available for Nori CLI,
//! replacing the OpenAI/Codex-specific update mechanisms.

/// Update action the CLI should perform after the TUI exits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateAction {
    /// Update via `npm install -g nori-ai-cli@latest`.
    NpmGlobalLatest,
    /// Update via `bun install -g nori-ai-cli@latest`.
    BunGlobalLatest,
    /// Update via `cargo install nori-cli`.
    CargoInstall,
}

impl UpdateAction {
    /// Returns the list of command-line arguments for invoking the update.
    pub fn command_args(self) -> (&'static str, &'static [&'static str]) {
        match self {
            UpdateAction::NpmGlobalLatest => ("npm", &["install", "-g", "nori-ai-cli"]),
            UpdateAction::BunGlobalLatest => ("bun", &["install", "-g", "nori-ai-cli"]),
            UpdateAction::CargoInstall => ("cargo", &["install", "nori-cli"]),
        }
    }

    /// Returns string representation of the command-line arguments for invoking the update.
    pub fn command_str(self) -> String {
        let (command, args) = self.command_args();
        shlex::try_join(std::iter::once(command).chain(args.iter().copied()))
            .unwrap_or_else(|_| format!("{command} {}", args.join(" ")))
    }
}

/// Detect the appropriate update action based on how Nori was installed.
pub(crate) fn get_update_action() -> Option<UpdateAction> {
    let exe = std::env::current_exe().unwrap_or_default();
    let managed_by_npm = std::env::var_os("NORI_MANAGED_BY_NPM").is_some();
    let managed_by_bun = std::env::var_os("NORI_MANAGED_BY_BUN").is_some();
    let managed_by_cargo = std::env::var_os("NORI_MANAGED_BY_CARGO").is_some();

    detect_update_action(&exe, managed_by_npm, managed_by_bun, managed_by_cargo)
}

fn detect_update_action(
    _current_exe: &std::path::Path,
    managed_by_npm: bool,
    managed_by_bun: bool,
    managed_by_cargo: bool,
) -> Option<UpdateAction> {
    if managed_by_npm {
        Some(UpdateAction::NpmGlobalLatest)
    } else if managed_by_bun {
        Some(UpdateAction::BunGlobalLatest)
    } else if managed_by_cargo {
        Some(UpdateAction::CargoInstall)
    } else {
        // Default to cargo install for Nori
        Some(UpdateAction::CargoInstall)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_update_action_without_env_mutation() {
        assert_eq!(
            detect_update_action(std::path::Path::new("/any/path"), false, false, false),
            Some(UpdateAction::CargoInstall)
        );
        assert_eq!(
            detect_update_action(std::path::Path::new("/any/path"), true, false, false),
            Some(UpdateAction::NpmGlobalLatest)
        );
        assert_eq!(
            detect_update_action(std::path::Path::new("/any/path"), false, true, false),
            Some(UpdateAction::BunGlobalLatest)
        );
        assert_eq!(
            detect_update_action(std::path::Path::new("/any/path"), false, false, true),
            Some(UpdateAction::CargoInstall)
        );
    }

    #[test]
    fn command_str_formats_correctly() {
        assert_eq!(
            UpdateAction::NpmGlobalLatest.command_str(),
            "npm install -g nori-ai-cli"
        );
        assert_eq!(
            UpdateAction::BunGlobalLatest.command_str(),
            "bun install -g nori-ai-cli"
        );
        assert_eq!(
            UpdateAction::CargoInstall.command_str(),
            "cargo install nori-cli"
        );
    }
}

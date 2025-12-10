//! Nori-specific update actions

/// Update action for Nori CLI
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateAction {
    /// Update via `npm install -g nori-ai-cli@latest`
    NpmGlobalLatest,
    /// Manual update (show instructions)
    Manual,
}

impl UpdateAction {
    /// Returns the list of command-line arguments for invoking the update.
    pub fn command_args(self) -> (&'static str, &'static [&'static str]) {
        match self {
            UpdateAction::NpmGlobalLatest => ("npm", &["install", "-g", "nori-ai-cli@latest"]),
            UpdateAction::Manual => (
                "echo",
                &["Please visit https://github.com/tilework-tech/nori-cli/releases"],
            ),
        }
    }

    /// Returns string representation of the command-line arguments for invoking the update.
    pub fn command_str(self) -> String {
        let (command, args) = self.command_args();
        shlex::try_join(std::iter::once(command).chain(args.iter().copied()))
            .unwrap_or_else(|_| format!("{command} {}", args.join(" ")))
    }
}

/// Returns the update action for the current installation.
///
/// Unlike the upstream version which returns `None` for unknown installations,
/// this always returns `Some()` because Nori supports a manual update fallback
/// that directs users to GitHub releases.
#[cfg(not(debug_assertions))]
pub(crate) fn get_update_action() -> Option<UpdateAction> {
    let managed_by_npm = std::env::var_os("NORI_MANAGED_BY_NPM").is_some();

    if managed_by_npm {
        Some(UpdateAction::NpmGlobalLatest)
    } else {
        // For other installations, show manual update option
        Some(UpdateAction::Manual)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn npm_update_command_is_correct() {
        let action = UpdateAction::NpmGlobalLatest;
        let (cmd, args) = action.command_args();
        assert_eq!(cmd, "npm");
        assert_eq!(args, &["install", "-g", "nori-ai-cli@latest"]);
    }

    #[test]
    fn manual_update_command_shows_url() {
        let action = UpdateAction::Manual;
        let (cmd, args) = action.command_args();
        assert_eq!(cmd, "echo");
        assert!(args[0].contains("tilework-tech/nori-cli"));
    }
}

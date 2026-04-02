#[cfg(windows)]
#[path = "windows_dangerous_commands.rs"]
mod windows_dangerous_commands;

#[cfg(test)]
mod tests {
    use codex_protocol::protocol::AskForApproval;
    use codex_protocol::protocol::SandboxPolicy;

    use crate::bash::parse_shell_lc_plain_commands;
    use crate::is_safe_command::is_known_safe_command;
    use crate::sandboxing::SandboxPermissions;

    fn requires_initial_appoval(
        policy: AskForApproval,
        sandbox_policy: &SandboxPolicy,
        command: &[String],
        sandbox_permissions: SandboxPermissions,
    ) -> bool {
        if is_known_safe_command(command) {
            return false;
        }
        match policy {
            AskForApproval::Never | AskForApproval::OnFailure => false,
            AskForApproval::OnRequest => {
                if matches!(sandbox_policy, SandboxPolicy::DangerFullAccess) {
                    return command_might_be_dangerous(command);
                }

                if sandbox_permissions.requires_escalated_permissions() {
                    return true;
                }
                command_might_be_dangerous(command)
            }
            AskForApproval::UnlessTrusted => !is_known_safe_command(command),
        }
    }

    fn command_might_be_dangerous(command: &[String]) -> bool {
        #[cfg(windows)]
        {
            if super::windows_dangerous_commands::is_dangerous_command_windows(command) {
                return true;
            }
        }

        if is_dangerous_to_call_with_exec(command) {
            return true;
        }

        if let Some(all_commands) = parse_shell_lc_plain_commands(command)
            && all_commands
                .iter()
                .any(|cmd| is_dangerous_to_call_with_exec(cmd))
        {
            return true;
        }

        false
    }

    fn is_dangerous_to_call_with_exec(command: &[String]) -> bool {
        let cmd0 = command.first().map(String::as_str);

        match cmd0 {
            Some(cmd) if cmd.ends_with("git") || cmd.ends_with("/git") => {
                matches!(command.get(1).map(String::as_str), Some("reset" | "rm"))
            }

            Some("rm") => matches!(command.get(1).map(String::as_str), Some("-f" | "-rf")),

            Some("sudo") => is_dangerous_to_call_with_exec(&command[1..]),

            _ => false,
        }
    }

    fn vec_str(items: &[&str]) -> Vec<String> {
        items.iter().map(std::string::ToString::to_string).collect()
    }

    #[allow(unused)]
    fn _requires_initial_appoval_compiles() {
        // Ensure requires_initial_appoval is used so the function doesn't warn.
        let _ = requires_initial_appoval(
            AskForApproval::OnRequest,
            &SandboxPolicy::ReadOnly,
            &[],
            SandboxPermissions::UseDefault,
        );
    }

    #[test]
    fn git_reset_is_dangerous() {
        assert!(command_might_be_dangerous(&vec_str(&["git", "reset"])));
    }

    #[test]
    fn bash_git_reset_is_dangerous() {
        assert!(command_might_be_dangerous(&vec_str(&[
            "bash",
            "-lc",
            "git reset --hard"
        ])));
    }

    #[test]
    fn zsh_git_reset_is_dangerous() {
        assert!(command_might_be_dangerous(&vec_str(&[
            "zsh",
            "-lc",
            "git reset --hard"
        ])));
    }

    #[test]
    fn git_status_is_not_dangerous() {
        assert!(!command_might_be_dangerous(&vec_str(&["git", "status"])));
    }

    #[test]
    fn bash_git_status_is_not_dangerous() {
        assert!(!command_might_be_dangerous(&vec_str(&[
            "bash",
            "-lc",
            "git status"
        ])));
    }

    #[test]
    fn sudo_git_reset_is_dangerous() {
        assert!(command_might_be_dangerous(&vec_str(&[
            "sudo", "git", "reset", "--hard"
        ])));
    }

    #[test]
    fn usr_bin_git_is_dangerous() {
        assert!(command_might_be_dangerous(&vec_str(&[
            "/usr/bin/git",
            "reset",
            "--hard"
        ])));
    }

    #[test]
    fn rm_rf_is_dangerous() {
        assert!(command_might_be_dangerous(&vec_str(&["rm", "-rf", "/"])));
    }

    #[test]
    fn rm_f_is_dangerous() {
        assert!(command_might_be_dangerous(&vec_str(&["rm", "-f", "/"])));
    }
}

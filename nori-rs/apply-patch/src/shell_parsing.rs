use super::*;

fn classify_shell_name(shell: &str) -> Option<String> {
    std::path::Path::new(shell)
        .file_stem()
        .and_then(|name| name.to_str())
        .map(str::to_ascii_lowercase)
}

fn classify_shell(shell: &str, flag: &str) -> Option<ApplyPatchShell> {
    classify_shell_name(shell).and_then(|name| match name.as_str() {
        "bash" | "zsh" | "sh" if flag == "-lc" => Some(ApplyPatchShell::Unix),
        "pwsh" | "powershell" if flag.eq_ignore_ascii_case("-command") => {
            Some(ApplyPatchShell::PowerShell)
        }
        "cmd" if flag.eq_ignore_ascii_case("/c") => Some(ApplyPatchShell::Cmd),
        _ => None,
    })
}

fn can_skip_flag(shell: &str, flag: &str) -> bool {
    classify_shell_name(shell).is_some_and(|name| {
        matches!(name.as_str(), "pwsh" | "powershell") && flag.eq_ignore_ascii_case("-noprofile")
    })
}

pub(crate) fn parse_shell_script(argv: &[String]) -> Option<(ApplyPatchShell, &str)> {
    match argv {
        [shell, flag, script] => classify_shell(shell, flag).map(|shell_type| {
            let script = script.as_str();
            (shell_type, script)
        }),
        [shell, skip_flag, flag, script] if can_skip_flag(shell, skip_flag) => {
            classify_shell(shell, flag).map(|shell_type| {
                let script = script.as_str();
                (shell_type, script)
            })
        }
        _ => None,
    }
}

pub(crate) fn extract_apply_patch_from_shell(
    shell: ApplyPatchShell,
    script: &str,
) -> std::result::Result<(String, Option<String>), ExtractHeredocError> {
    match shell {
        ApplyPatchShell::Unix | ApplyPatchShell::PowerShell | ApplyPatchShell::Cmd => {
            heredoc::extract_apply_patch_from_bash(script)
        }
    }
}

pub fn maybe_parse_apply_patch(argv: &[String]) -> MaybeApplyPatch {
    match argv {
        // Direct invocation: apply_patch <patch>
        [cmd, body] if APPLY_PATCH_COMMANDS.contains(&cmd.as_str()) => match parse_patch(body) {
            Ok(source) => MaybeApplyPatch::Body(source),
            Err(e) => MaybeApplyPatch::PatchParseError(e),
        },
        // Shell heredoc form: (optional `cd <path> &&`) apply_patch <<'EOF' ...
        _ => match parse_shell_script(argv) {
            Some((shell, script)) => match extract_apply_patch_from_shell(shell, script) {
                Ok((body, workdir)) => match parse_patch(&body) {
                    Ok(mut source) => {
                        source.workdir = workdir;
                        MaybeApplyPatch::Body(source)
                    }
                    Err(e) => MaybeApplyPatch::PatchParseError(e),
                },
                Err(ExtractHeredocError::CommandDidNotStartWithApplyPatch) => {
                    MaybeApplyPatch::NotApplyPatch
                }
                Err(e) => MaybeApplyPatch::ShellParseError(e),
            },
            None => MaybeApplyPatch::NotApplyPatch,
        },
    }
}

//! Sandbox transformation for ACP agent subprocesses.
//!
//! This module provides platform-specific sandbox wrapping for ACP agent
//! subprocesses. On macOS, agents are wrapped with Seatbelt (`sandbox-exec`).
//! On Linux, agents are wrapped with Landlock+seccomp via `codex-linux-sandbox`.
//! On Windows, restricted tokens are used (handled separately).

use std::path::Path;

use codex_protocol::protocol::SandboxPolicy;

/// The type of sandbox to apply to a subprocess.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxType {
    /// No sandboxing applied.
    None,
    /// macOS Seatbelt sandbox via `/usr/bin/sandbox-exec`.
    MacosSeatbelt,
    /// Linux Landlock + seccomp sandbox via `codex-linux-sandbox`.
    LinuxSeccomp,
    /// Windows restricted token sandbox (handled in-process, not via command wrapping).
    WindowsRestrictedToken,
}

/// Result of transforming a command for sandbox execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxedCommand {
    /// The executable to run (may be sandbox wrapper or original command).
    pub program: String,
    /// Arguments to pass to the executable.
    pub args: Vec<String>,
    /// Optional override for argv[0] (used for Linux arg0 dispatch).
    pub arg0: Option<String>,
    /// The sandbox type that was applied.
    pub sandbox_type: SandboxType,
}

/// Returns the appropriate sandbox type for the current platform.
///
/// Returns `None` if no sandbox is available for the platform.
pub fn get_platform_sandbox() -> Option<SandboxType> {
    #[cfg(target_os = "macos")]
    {
        Some(SandboxType::MacosSeatbelt)
    }
    #[cfg(target_os = "linux")]
    {
        Some(SandboxType::LinuxSeccomp)
    }
    #[cfg(target_os = "windows")]
    {
        // Windows sandbox requires explicit enablement and uses a different
        // execution path (in-process token restriction, not command wrapping).
        // For now, return None to indicate command wrapping is not used.
        None
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        None
    }
}

/// Transforms a command to run under the appropriate platform sandbox.
///
/// # Arguments
/// * `program` - The original program to execute
/// * `args` - Arguments to pass to the program
/// * `sandbox_policy` - The sandbox policy defining allowed operations
/// * `cwd` - The working directory for sandbox policy resolution
/// * `codex_linux_sandbox_exe` - Path to the `codex-linux-sandbox` binary (required on Linux)
///
/// # Returns
/// A `SandboxedCommand` containing the transformed command, or an error if
/// sandboxing could not be applied (e.g., missing Linux sandbox binary).
pub fn transform_command_for_sandbox(
    program: &str,
    args: &[String],
    sandbox_policy: &SandboxPolicy,
    cwd: &Path,
    codex_linux_sandbox_exe: Option<&Path>,
) -> Result<SandboxedCommand, SandboxError> {
    // Build the full command vector (program + args)
    let command: Vec<String> = std::iter::once(program.to_string())
        .chain(args.iter().cloned())
        .collect();

    // Check if sandbox should be bypassed
    if matches!(sandbox_policy, SandboxPolicy::DangerFullAccess) {
        return Ok(SandboxedCommand {
            program: program.to_string(),
            args: args.to_vec(),
            arg0: None,
            sandbox_type: SandboxType::None,
        });
    }

    match get_platform_sandbox() {
        None => {
            // No sandbox available - run without sandboxing
            Ok(SandboxedCommand {
                program: program.to_string(),
                args: args.to_vec(),
                arg0: None,
                sandbox_type: SandboxType::None,
            })
        }

        Some(SandboxType::MacosSeatbelt) => {
            #[cfg(target_os = "macos")]
            {
                let seatbelt_args = create_seatbelt_command_args(command, sandbox_policy, cwd);
                Ok(SandboxedCommand {
                    program: MACOS_PATH_TO_SEATBELT_EXECUTABLE.to_string(),
                    args: seatbelt_args,
                    arg0: None,
                    sandbox_type: SandboxType::MacosSeatbelt,
                })
            }
            #[cfg(not(target_os = "macos"))]
            {
                // Should not reach here due to get_platform_sandbox() logic
                Ok(SandboxedCommand {
                    program: program.to_string(),
                    args: args.to_vec(),
                    arg0: None,
                    sandbox_type: SandboxType::None,
                })
            }
        }

        Some(SandboxType::LinuxSeccomp) => {
            match codex_linux_sandbox_exe {
                Some(exe) => {
                    let linux_args =
                        create_linux_sandbox_command_args(command, sandbox_policy, cwd);
                    Ok(SandboxedCommand {
                        program: exe.to_string_lossy().to_string(),
                        args: linux_args,
                        arg0: Some("codex-linux-sandbox".to_string()),
                        sandbox_type: SandboxType::LinuxSeccomp,
                    })
                }
                None => {
                    // Graceful degradation: if the Linux sandbox binary path is not provided,
                    // run without OS-level sandboxing. This allows ACP to work on Linux even
                    // before the codex-linux-sandbox binary path is properly configured.
                    // Application-level path restrictions in write_text_file() still provide
                    // defense-in-depth protection.
                    tracing::warn!(
                        "codex-linux-sandbox binary path not provided; running without OS-level sandbox"
                    );
                    Ok(SandboxedCommand {
                        program: program.to_string(),
                        args: args.to_vec(),
                        arg0: None,
                        sandbox_type: SandboxType::None,
                    })
                }
            }
        }

        Some(SandboxType::WindowsRestrictedToken) => {
            // Windows uses in-process token restriction, not command wrapping.
            // Return the original command; the caller must handle Windows sandbox separately.
            Ok(SandboxedCommand {
                program: program.to_string(),
                args: args.to_vec(),
                arg0: None,
                sandbox_type: SandboxType::WindowsRestrictedToken,
            })
        }

        Some(SandboxType::None) => Ok(SandboxedCommand {
            program: program.to_string(),
            args: args.to_vec(),
            arg0: None,
            sandbox_type: SandboxType::None,
        }),
    }
}

/// Errors that can occur during sandbox transformation.
///
/// Note: Currently no errors are returned - the implementation uses graceful
/// degradation to unsandboxed execution when sandbox setup fails. This error
/// type is kept for future use when stricter sandbox enforcement may be added.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SandboxError {
    // Currently empty - using graceful degradation instead of errors.
    // Future variants might include:
    // - SandboxBinaryNotFound
    // - SandboxBinaryNotExecutable
    // - PolicySerializationFailed
}

// ============================================================================
// Platform-specific sandbox command generation
// ============================================================================

/// Creates command arguments for running under macOS Seatbelt sandbox.
/// Delegates to codex-core's seatbelt implementation and uses its path constant.
#[cfg(target_os = "macos")]
fn create_seatbelt_command_args(
    command: Vec<String>,
    sandbox_policy: &SandboxPolicy,
    sandbox_policy_cwd: &Path,
) -> Vec<String> {
    codex_core::seatbelt::create_seatbelt_command_args(command, sandbox_policy, sandbox_policy_cwd)
}

/// Re-export the seatbelt executable path from codex-core for macOS.
#[cfg(target_os = "macos")]
pub use codex_core::seatbelt::MACOS_PATH_TO_SEATBELT_EXECUTABLE;

/// Creates command arguments for running under Linux Landlock+seccomp sandbox.
fn create_linux_sandbox_command_args(
    command: Vec<String>,
    sandbox_policy: &SandboxPolicy,
    sandbox_policy_cwd: &Path,
) -> Vec<String> {
    // Serialize sandbox policy to JSON for the helper binary
    #[allow(clippy::expect_used)]
    let sandbox_policy_cwd_str = sandbox_policy_cwd
        .to_str()
        .expect("cwd must be valid UTF-8")
        .to_string();

    #[allow(clippy::expect_used)]
    let sandbox_policy_json =
        serde_json::to_string(sandbox_policy).expect("Failed to serialize SandboxPolicy to JSON");

    let mut linux_cmd: Vec<String> = vec![
        "--sandbox-policy-cwd".to_string(),
        sandbox_policy_cwd_str,
        "--sandbox-policy".to_string(),
        sandbox_policy_json,
        "--".to_string(),
    ];

    linux_cmd.extend(command);
    linux_cmd
}

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(unused_imports)]
    use std::path::PathBuf;

    #[test]
    fn test_transform_with_danger_full_access_bypasses_sandbox() {
        let policy = SandboxPolicy::DangerFullAccess;
        let cwd = PathBuf::from("/tmp/test");

        let result = transform_command_for_sandbox(
            "npx",
            &["@anthropic/claude-code".to_string()],
            &policy,
            &cwd,
            None,
        )
        .unwrap();

        assert_eq!(result.program, "npx");
        assert_eq!(result.args, vec!["@anthropic/claude-code"]);
        assert_eq!(result.arg0, None);
        assert_eq!(result.sandbox_type, SandboxType::None);
    }

    #[test]
    fn test_get_platform_sandbox_returns_expected_type() {
        let sandbox = get_platform_sandbox();

        #[cfg(target_os = "macos")]
        assert_eq!(sandbox, Some(SandboxType::MacosSeatbelt));

        #[cfg(target_os = "linux")]
        assert_eq!(sandbox, Some(SandboxType::LinuxSeccomp));

        #[cfg(target_os = "windows")]
        assert_eq!(sandbox, None); // Windows uses different mechanism

        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        assert_eq!(sandbox, None);
    }

    #[test]
    fn test_linux_sandbox_graceful_degradation_without_exe_path() {
        // This test is only meaningful on Linux where we expect LinuxSeccomp sandbox
        #[cfg(target_os = "linux")]
        {
            let policy = SandboxPolicy::new_workspace_write_policy();
            let cwd = PathBuf::from("/tmp/test");

            // When no executable path is provided, should gracefully degrade
            // to unsandboxed execution instead of failing
            let result = transform_command_for_sandbox(
                "npx",
                &["@anthropic/claude-code".to_string()],
                &policy,
                &cwd,
                None, // No executable path provided
            )
            .unwrap();

            // Should return original command with SandboxType::None
            assert_eq!(result.program, "npx");
            assert_eq!(result.args, vec!["@anthropic/claude-code"]);
            assert_eq!(result.sandbox_type, SandboxType::None);
        }
    }

    #[test]
    fn test_linux_sandbox_wraps_command_correctly() {
        let policy = SandboxPolicy::WorkspaceWrite {
            writable_roots: vec![],
            network_access: false,
            exclude_tmpdir_env_var: true,
            exclude_slash_tmp: true,
        };
        let cwd = PathBuf::from("/home/user/project");

        // Create the expected linux args directly to verify format
        let command = vec!["npx".to_string(), "@anthropic/claude-code".to_string()];
        let linux_args = create_linux_sandbox_command_args(command, &policy, &cwd);

        // Verify the structure of linux args
        assert_eq!(linux_args[0], "--sandbox-policy-cwd");
        assert_eq!(linux_args[1], "/home/user/project");
        assert_eq!(linux_args[2], "--sandbox-policy");
        // linux_args[3] is the JSON policy
        assert_eq!(linux_args[4], "--");
        assert_eq!(linux_args[5], "npx");
        assert_eq!(linux_args[6], "@anthropic/claude-code");

        // Verify the JSON policy can be parsed back
        let parsed_policy: SandboxPolicy = serde_json::from_str(&linux_args[3]).unwrap();
        assert_eq!(parsed_policy, policy);
    }

    #[test]
    fn test_read_only_policy_enables_sandbox() {
        let policy = SandboxPolicy::ReadOnly;
        let cwd = PathBuf::from("/tmp/test");

        // On platforms with sandbox, ReadOnly should enable it
        #[cfg(target_os = "linux")]
        {
            let linux_exe = PathBuf::from("/usr/local/bin/codex-linux-sandbox");
            let result = transform_command_for_sandbox(
                "echo",
                &["hello".to_string()],
                &policy,
                &cwd,
                Some(&linux_exe),
            )
            .unwrap();
            assert_eq!(result.sandbox_type, SandboxType::LinuxSeccomp);
        }

        #[cfg(target_os = "macos")]
        {
            let result =
                transform_command_for_sandbox("echo", &["hello".to_string()], &policy, &cwd, None)
                    .unwrap();
            assert_eq!(result.sandbox_type, SandboxType::MacosSeatbelt);
        }
    }
}

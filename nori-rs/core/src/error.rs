use crate::exec::ExecToolCallOutput;
use crate::truncate::TruncationPolicy;
use crate::truncate::truncate_text;
use serde_json;
use std::io;
use thiserror::Error;
use tokio::task::JoinError;

pub type Result<T> = std::result::Result<T, CodexErr>;

/// Limit UI error messages to a reasonable size while keeping useful context.
const ERROR_MESSAGE_UI_MAX_BYTES: usize = 2 * 1024; // 2 KiB

#[derive(Error, Debug)]
pub enum SandboxErr {
    /// Error from sandbox execution
    #[error(
        "sandbox denied exec error, exit code: {}, stdout: {}, stderr: {}",
        .output.exit_code, .output.stdout.text, .output.stderr.text
    )]
    Denied { output: Box<ExecToolCallOutput> },

    /// Error from linux seccomp filter setup
    #[cfg(target_os = "linux")]
    #[error("seccomp setup error")]
    SeccompInstall(#[from] seccompiler::Error),

    /// Error from linux seccomp backend
    #[cfg(target_os = "linux")]
    #[error("seccomp backend error")]
    SeccompBackend(#[from] seccompiler::BackendError),

    /// Command timed out
    #[error("command timed out")]
    Timeout { output: Box<ExecToolCallOutput> },

    /// Command was killed by a signal
    #[error("command was killed by a signal")]
    Signal(i32),

    /// Error from linux landlock
    #[error("Landlock was not able to fully enforce all sandbox rules")]
    LandlockRestrict,
}

#[derive(Error, Debug)]
pub enum CodexErr {
    /// Sandbox error
    #[error("sandbox error: {0}")]
    Sandbox(#[from] SandboxErr),

    #[error("codex-linux-sandbox was required but not provided")]
    LandlockSandboxExecutableNotProvided,

    #[error("unsupported operation: {0}")]
    UnsupportedOperation(String),

    #[error("{0}")]
    RefreshTokenFailed(RefreshTokenFailedError),

    #[error("Fatal error: {0}")]
    Fatal(String),

    // -----------------------------------------------------------------
    // Automatic conversions for common external error types
    // -----------------------------------------------------------------
    #[error(transparent)]
    Io(#[from] io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[cfg(target_os = "linux")]
    #[error(transparent)]
    LandlockRuleset(#[from] landlock::RulesetError),

    #[cfg(target_os = "linux")]
    #[error(transparent)]
    LandlockPathFd(#[from] landlock::PathFdError),

    #[error(transparent)]
    TokioJoin(#[from] JoinError),

    #[error("{0}")]
    EnvVar(EnvVarError),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct RefreshTokenFailedError {
    pub reason: RefreshTokenFailedReason,
    pub message: String,
}

impl RefreshTokenFailedError {
    pub fn new(reason: RefreshTokenFailedReason, message: impl Into<String>) -> Self {
        Self {
            reason,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshTokenFailedReason {
    Expired,
    Exhausted,
    Revoked,
    Other,
}

#[derive(Debug)]
pub struct EnvVarError {
    /// Name of the environment variable that is missing.
    pub var: String,

    /// Optional instructions to help the user get a valid value for the
    /// variable and set it.
    pub instructions: Option<String>,
}

impl std::fmt::Display for EnvVarError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Missing environment variable: `{}`.", self.var)?;
        if let Some(instructions) = &self.instructions {
            write!(f, " {instructions}")?;
        }
        Ok(())
    }
}

impl CodexErr {
    /// Minimal shim so that existing `e.downcast_ref::<CodexErr>()` checks continue to compile
    /// after replacing `anyhow::Error` in the return signature. This mirrors the behavior of
    /// `anyhow::Error::downcast_ref` but works directly on our concrete enum.
    pub fn downcast_ref<T: std::any::Any>(&self) -> Option<&T> {
        (self as &dyn std::any::Any).downcast_ref::<T>()
    }
}

pub fn get_error_message_ui(e: &CodexErr) -> String {
    let message = match e {
        CodexErr::Sandbox(SandboxErr::Denied { output }) => {
            let aggregated = output.aggregated_output.text.trim();
            if !aggregated.is_empty() {
                output.aggregated_output.text.clone()
            } else {
                let stderr = output.stderr.text.trim();
                let stdout = output.stdout.text.trim();
                match (stderr.is_empty(), stdout.is_empty()) {
                    (false, false) => format!("{stderr}\n{stdout}"),
                    (false, true) => output.stderr.text.clone(),
                    (true, false) => output.stdout.text.clone(),
                    (true, true) => format!(
                        "command failed inside sandbox with exit code {}",
                        output.exit_code
                    ),
                }
            }
        }
        // Timeouts are not sandbox errors from a UX perspective; present them plainly
        CodexErr::Sandbox(SandboxErr::Timeout { output }) => {
            format!(
                "error: command timed out after {} ms",
                output.duration.as_millis()
            )
        }
        _ => e.to_string(),
    };

    truncate_text(
        &message,
        TruncationPolicy::Bytes(ERROR_MESSAGE_UI_MAX_BYTES),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::StreamOutput;
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    #[test]
    fn sandbox_denied_uses_aggregated_output_when_stderr_empty() {
        let output = ExecToolCallOutput {
            exit_code: 77,
            stdout: StreamOutput::new(String::new()),
            stderr: StreamOutput::new(String::new()),
            aggregated_output: StreamOutput::new("aggregate detail".to_string()),
            duration: Duration::from_millis(10),
            timed_out: false,
        };
        let err = CodexErr::Sandbox(SandboxErr::Denied {
            output: Box::new(output),
        });
        assert_eq!(get_error_message_ui(&err), "aggregate detail");
    }

    #[test]
    fn sandbox_denied_reports_both_streams_when_available() {
        let output = ExecToolCallOutput {
            exit_code: 9,
            stdout: StreamOutput::new("stdout detail".to_string()),
            stderr: StreamOutput::new("stderr detail".to_string()),
            aggregated_output: StreamOutput::new(String::new()),
            duration: Duration::from_millis(10),
            timed_out: false,
        };
        let err = CodexErr::Sandbox(SandboxErr::Denied {
            output: Box::new(output),
        });
        assert_eq!(get_error_message_ui(&err), "stderr detail\nstdout detail");
    }

    #[test]
    fn sandbox_denied_reports_stdout_when_no_stderr() {
        let output = ExecToolCallOutput {
            exit_code: 11,
            stdout: StreamOutput::new("stdout only".to_string()),
            stderr: StreamOutput::new(String::new()),
            aggregated_output: StreamOutput::new(String::new()),
            duration: Duration::from_millis(8),
            timed_out: false,
        };
        let err = CodexErr::Sandbox(SandboxErr::Denied {
            output: Box::new(output),
        });
        assert_eq!(get_error_message_ui(&err), "stdout only");
    }

    #[test]
    fn sandbox_denied_reports_exit_code_when_no_output_available() {
        let output = ExecToolCallOutput {
            exit_code: 13,
            stdout: StreamOutput::new(String::new()),
            stderr: StreamOutput::new(String::new()),
            aggregated_output: StreamOutput::new(String::new()),
            duration: Duration::from_millis(5),
            timed_out: false,
        };
        let err = CodexErr::Sandbox(SandboxErr::Denied {
            output: Box::new(output),
        });
        assert_eq!(
            get_error_message_ui(&err),
            "command failed inside sandbox with exit code 13"
        );
    }
}

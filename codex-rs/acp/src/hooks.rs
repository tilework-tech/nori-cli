//! Session lifecycle hooks
//!
//! Provides execution of user-configured scripts at session start and end.
//! Scripts are configured in `config.toml` under the `[hooks]` section:
//!
//! ```toml
//! [hooks]
//! session_start = ["~/.nori/cli/hooks/start.sh"]
//! session_end = ["~/.nori/cli/hooks/end.sh"]
//! ```

use std::path::Path;
use std::time::Duration;

use tracing::warn;

/// Result of executing a single hook script.
#[derive(Debug, Clone)]
pub struct HookResult {
    /// Path to the script that was executed.
    pub path: String,
    /// Whether the script succeeded (exit code 0).
    pub success: bool,
    /// Captured stdout on success.
    pub output: Option<String>,
    /// Error message on failure.
    pub error: Option<String>,
}

/// A parsed line from hook script stdout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookOutputLine {
    /// Plain log line (no prefix) — goes to tracing.
    Log(String),
    /// `::output::` prefix — bare text displayed in the TUI.
    Output(String),
    /// `::output-warn::` prefix — yellow warning text in the TUI.
    OutputWarn(String),
    /// `::output-error::` prefix — red error text in the TUI.
    OutputError(String),
    /// `::context::` prefix — prepended to the next agent prompt.
    Context(String),
}

/// Parse hook stdout into typed output lines based on prefix routing.
///
/// Each line is examined for a known prefix. Lines without a recognized
/// prefix are treated as plain log output. Empty lines are skipped.
pub fn parse_hook_output(output: &str) -> Vec<HookOutputLine> {
    output
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            if let Some(rest) = line.strip_prefix("::output-warn::") {
                HookOutputLine::OutputWarn(rest.to_string())
            } else if let Some(rest) = line.strip_prefix("::output-error::") {
                HookOutputLine::OutputError(rest.to_string())
            } else if let Some(rest) = line.strip_prefix("::output::") {
                HookOutputLine::Output(rest.to_string())
            } else if let Some(rest) = line.strip_prefix("::context::") {
                HookOutputLine::Context(rest.to_string())
            } else {
                HookOutputLine::Log(line.to_string())
            }
        })
        .collect()
}

/// Determine the interpreter for a script based on its file extension.
/// Returns `None` if the script should be executed directly (no recognized extension).
fn interpreter_for(path: &Path) -> Option<&'static str> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("sh") => Some("bash"),
        Some("py") => Some("python3"),
        Some("js") => Some("node"),
        _ => None,
    }
}

/// Execute a list of hook scripts sequentially.
///
/// Each script is run with the given timeout. Failures are logged but do not
/// prevent subsequent hooks from executing. Returns a result for each hook.
pub async fn execute_hooks(hooks: &[impl AsRef<Path>], timeout: Duration) -> Vec<HookResult> {
    let mut results = Vec::with_capacity(hooks.len());

    for hook_path in hooks {
        let path = hook_path.as_ref();
        let path_str = path.display().to_string();

        if !path.exists() {
            let msg = format!("Hook script not found: {path_str}");
            warn!("{msg}");
            results.push(HookResult {
                path: path_str,
                success: false,
                output: None,
                error: Some(msg),
            });
            continue;
        }

        let mut cmd = if let Some(interpreter) = interpreter_for(path) {
            let mut c = tokio::process::Command::new(interpreter);
            c.arg(path);
            c
        } else {
            tokio::process::Command::new(path)
        };

        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        cmd.stdin(std::process::Stdio::null());
        cmd.kill_on_drop(true);

        let child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => {
                let msg = format!("Failed to spawn hook '{path_str}': {e}");
                warn!("{msg}");
                results.push(HookResult {
                    path: path_str,
                    success: false,
                    output: None,
                    error: Some(msg),
                });
                continue;
            }
        };

        let result = tokio::time::timeout(timeout, child.wait_with_output()).await;

        match result {
            Ok(Ok(output)) => {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    results.push(HookResult {
                        path: path_str,
                        success: true,
                        output: if stdout.is_empty() {
                            None
                        } else {
                            Some(stdout)
                        },
                        error: None,
                    });
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    let code = output
                        .status
                        .code()
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    let msg = format!("Hook '{path_str}' failed with exit code {code}: {stderr}");
                    warn!("{msg}");
                    results.push(HookResult {
                        path: path_str,
                        success: false,
                        output: None,
                        error: Some(msg),
                    });
                }
            }
            Ok(Err(e)) => {
                let msg = format!("Hook '{path_str}' I/O error: {e}");
                warn!("{msg}");
                results.push(HookResult {
                    path: path_str,
                    success: false,
                    output: None,
                    error: Some(msg),
                });
            }
            Err(_) => {
                let msg = format!(
                    "Hook '{path_str}' timed out after {:.1}s",
                    timeout.as_secs_f64()
                );
                warn!("{msg}");
                results.push(HookResult {
                    path: path_str,
                    success: false,
                    output: None,
                    error: Some(msg),
                });
            }
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::fs;
    use tempfile::tempdir;

    // ---- parse_hook_output tests ----

    #[test]
    fn parse_hook_output_plain_lines_become_log() {
        let output = "hello world\ngoodbye\n";
        let lines = parse_hook_output(output);
        assert_eq!(
            lines,
            vec![
                HookOutputLine::Log("hello world".to_string()),
                HookOutputLine::Log("goodbye".to_string()),
            ]
        );
    }

    #[test]
    fn parse_hook_output_output_prefix() {
        let output = "::output::some bare text\n";
        let lines = parse_hook_output(output);
        assert_eq!(
            lines,
            vec![HookOutputLine::Output("some bare text".to_string())]
        );
    }

    #[test]
    fn parse_hook_output_warn_prefix() {
        let output = "::output-warn::watch out\n";
        let lines = parse_hook_output(output);
        assert_eq!(
            lines,
            vec![HookOutputLine::OutputWarn("watch out".to_string())]
        );
    }

    #[test]
    fn parse_hook_output_error_prefix() {
        let output = "::output-error::something broke\n";
        let lines = parse_hook_output(output);
        assert_eq!(
            lines,
            vec![HookOutputLine::OutputError("something broke".to_string())]
        );
    }

    #[test]
    fn parse_hook_output_context_prefix() {
        let output = "::context::remember this\n";
        let lines = parse_hook_output(output);
        assert_eq!(
            lines,
            vec![HookOutputLine::Context("remember this".to_string())]
        );
    }

    #[test]
    fn parse_hook_output_mixed_lines() {
        let output = "plain log line\n::output::visible text\n::context::ctx data\n::output-warn::warning msg\n::output-error::error msg\n";
        let lines = parse_hook_output(output);
        assert_eq!(
            lines,
            vec![
                HookOutputLine::Log("plain log line".to_string()),
                HookOutputLine::Output("visible text".to_string()),
                HookOutputLine::Context("ctx data".to_string()),
                HookOutputLine::OutputWarn("warning msg".to_string()),
                HookOutputLine::OutputError("error msg".to_string()),
            ]
        );
    }

    #[test]
    fn parse_hook_output_empty_after_prefix() {
        let output = "::output::\n";
        let lines = parse_hook_output(output);
        assert_eq!(lines, vec![HookOutputLine::Output(String::new())]);
    }

    #[test]
    fn parse_hook_output_skips_blank_lines() {
        let output = "line1\n\nline2\n";
        let lines = parse_hook_output(output);
        assert_eq!(
            lines,
            vec![
                HookOutputLine::Log("line1".to_string()),
                HookOutputLine::Log("line2".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn execute_hooks_runs_successful_script() {
        let tmp = tempdir().unwrap();
        let script = tmp.path().join("hook.sh");
        fs::write(&script, "#!/bin/bash\necho 'hook ran'").unwrap();

        let results = execute_hooks(&[&script], Duration::from_secs(5)).await;

        assert_eq!(results.len(), 1);
        assert!(results[0].success);
        assert_eq!(results[0].output.as_deref(), Some("hook ran\n"));
        assert!(results[0].error.is_none());
    }

    #[tokio::test]
    async fn execute_hooks_captures_failure() {
        let tmp = tempdir().unwrap();
        let script = tmp.path().join("fail.sh");
        fs::write(&script, "#!/bin/bash\necho 'oops' >&2\nexit 1").unwrap();

        let results = execute_hooks(&[&script], Duration::from_secs(5)).await;

        assert_eq!(results.len(), 1);
        assert!(!results[0].success);
        assert!(results[0].error.as_ref().unwrap().contains("exit code"));
        assert!(results[0].error.as_ref().unwrap().contains("oops"));
    }

    #[tokio::test]
    async fn execute_hooks_handles_missing_script() {
        let results =
            execute_hooks(&[Path::new("/nonexistent/hook.sh")], Duration::from_secs(5)).await;

        assert_eq!(results.len(), 1);
        assert!(!results[0].success);
        assert!(results[0].error.as_ref().unwrap().contains("not found"));
    }

    #[tokio::test]
    async fn execute_hooks_respects_timeout() {
        let tmp = tempdir().unwrap();
        let script = tmp.path().join("slow.sh");
        fs::write(&script, "#!/bin/bash\nsleep 60").unwrap();

        let results = execute_hooks(&[&script], Duration::from_millis(100)).await;

        assert_eq!(results.len(), 1);
        assert!(!results[0].success);
        assert!(
            results[0]
                .error
                .as_ref()
                .unwrap()
                .to_lowercase()
                .contains("timed out")
        );
    }

    #[tokio::test]
    async fn execute_hooks_runs_in_order() {
        let tmp = tempdir().unwrap();
        let marker = tmp.path().join("order.txt");

        let script1 = tmp.path().join("first.sh");
        fs::write(
            &script1,
            format!("#!/bin/bash\necho 'first' >> '{}'", marker.display()),
        )
        .unwrap();

        let script2 = tmp.path().join("second.sh");
        fs::write(
            &script2,
            format!("#!/bin/bash\necho 'second' >> '{}'", marker.display()),
        )
        .unwrap();

        let results = execute_hooks(&[&script1, &script2], Duration::from_secs(5)).await;

        assert_eq!(results.len(), 2);
        assert!(results[0].success);
        assert!(results[1].success);

        let contents = fs::read_to_string(&marker).unwrap();
        assert_eq!(contents, "first\nsecond\n");
    }

    #[tokio::test]
    async fn execute_hooks_continues_after_failure() {
        let tmp = tempdir().unwrap();

        let bad_script = tmp.path().join("bad.sh");
        fs::write(&bad_script, "#!/bin/bash\nexit 1").unwrap();

        let good_script = tmp.path().join("good.sh");
        fs::write(&good_script, "#!/bin/bash\necho 'ok'").unwrap();

        let results = execute_hooks(&[&bad_script, &good_script], Duration::from_secs(5)).await;

        assert_eq!(results.len(), 2);
        assert!(!results[0].success);
        assert!(results[1].success);
        assert_eq!(results[1].output.as_deref(), Some("ok\n"));
    }

    #[tokio::test]
    async fn execute_hooks_uses_python_interpreter() {
        let tmp = tempdir().unwrap();
        let script = tmp.path().join("hook.py");
        fs::write(&script, "print('python hook')").unwrap();

        let results = execute_hooks(&[&script], Duration::from_secs(5)).await;

        assert_eq!(results.len(), 1);
        assert!(results[0].success);
        assert_eq!(results[0].output.as_deref(), Some("python hook\n"));
    }

    #[tokio::test]
    async fn execute_hooks_with_prefixed_output_parses_correctly() {
        let tmp = tempdir().unwrap();
        let script = tmp.path().join("prefixed.sh");
        fs::write(
            &script,
            "#!/bin/bash\necho 'plain log'\necho '::output::hello user'\necho '::context::ctx data'\necho '::output-warn::be careful'\necho '::output-error::bad thing'",
        )
        .unwrap();

        let results = execute_hooks(&[&script], Duration::from_secs(5)).await;

        assert_eq!(results.len(), 1);
        assert!(results[0].success);
        let output = results[0].output.as_deref().unwrap();
        let parsed = parse_hook_output(output);
        assert_eq!(
            parsed,
            vec![
                HookOutputLine::Log("plain log".to_string()),
                HookOutputLine::Output("hello user".to_string()),
                HookOutputLine::Context("ctx data".to_string()),
                HookOutputLine::OutputWarn("be careful".to_string()),
                HookOutputLine::OutputError("bad thing".to_string()),
            ]
        );
    }
}

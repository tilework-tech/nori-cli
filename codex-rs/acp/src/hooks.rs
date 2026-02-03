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
}

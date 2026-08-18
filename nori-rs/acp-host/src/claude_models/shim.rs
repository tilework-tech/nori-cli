//! Generation of the `claude` wrapper that injects an expanded model list.
//!
//! The Claude ACP adapter resolves the Claude Code binary through
//! `CLAUDE_CODE_EXECUTABLE`. Pointing that at a generated wrapper lets nori add
//! `--settings <file>` to every invocation, which *merges* into the user's real
//! configuration rather than replacing it, so auth and existing settings are
//! untouched. The settings live in a file rather than inline JSON so that no
//! JSON ever passes through shell quoting.
//!
//! Both artifacts are shared by every concurrent nori session, so they are
//! published by atomic rename: a truncating write would let one session read a
//! half-written settings file, or hit `ETXTBSY` executing a half-written
//! wrapper, which would fail agent startup outright.

use std::path::Path;
use std::path::PathBuf;

use anyhow::Result;

/// Writes the settings file and wrapper script into `dir`, returning the path
/// to use as `CLAUDE_CODE_EXECUTABLE`.
#[cfg(unix)]
pub(super) fn write_shim(dir: &Path, claude_path: &Path, models: &[String]) -> Result<PathBuf> {
    std::fs::create_dir_all(dir)?;

    let settings_path = dir.join("claude-settings.json");
    publish(
        &settings_path,
        &serde_json::to_vec(&serde_json::json!({ "availableModels": models }))?,
        false,
    )?;

    let shim_path = dir.join("claude-with-models");
    let script = format!(
        "#!/bin/sh\nexec {} --settings {} \"$@\"\n",
        sh_quote(&claude_path.to_string_lossy()),
        sh_quote(&settings_path.to_string_lossy()),
    );
    publish(&shim_path, script.as_bytes(), true)?;

    Ok(shim_path)
}

/// Windows has no verified wrapper strategy: Node refuses to `spawn` a `.cmd`
/// without a shell since CVE-2024-27980, so a wrapper would break the Claude
/// agent rather than degrade. Report failure so the caller leaves the agent's
/// own model list in place.
#[cfg(not(unix))]
pub(super) fn write_shim(_dir: &Path, _claude_path: &Path, _models: &[String]) -> Result<PathBuf> {
    anyhow::bail!("model list expansion is only supported on unix")
}

/// Renders `value` as a single-quoted shell word. Unlike double quotes, single
/// quotes suppress `$`, backticks and backslashes, so paths cannot corrupt the
/// script or execute.
#[cfg(unix)]
fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// Installs `contents` at `path` via a private temporary file plus rename, so
/// readers and `execve` only ever observe a complete file.
#[cfg(unix)]
fn publish(path: &Path, contents: &[u8], executable: bool) -> Result<()> {
    if std::fs::read(path).is_ok_and(|existing| existing == contents) {
        return Ok(());
    }

    use std::os::unix::fs::PermissionsExt;

    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let staged = path.with_file_name(format!("{file_name}.{}.tmp", std::process::id()));

    std::fs::write(&staged, contents)?;
    let mode = if executable { 0o755 } else { 0o644 };
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(mode))?;
    std::fs::rename(&staged, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::fs;

    #[cfg(unix)]
    fn write_executable(path: &Path, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        fs::write(path, body).expect("write script");
        let mut perms = fs::metadata(path).expect("stat script").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).expect("chmod script");
    }

    #[cfg(unix)]
    #[test]
    fn shim_runs_claude_with_an_expanded_model_list_and_forwards_its_arguments() {
        let dir = tempfile::tempdir().expect("tempdir");
        let stub_claude = dir.path().join("claude-stub");
        write_executable(&stub_claude, "#!/bin/sh\nprintf '%s\\n' \"$@\"\n");

        let models = vec!["claude-opus-4-8".to_string(), "claude-sonnet-5".to_string()];
        let shim = write_shim(dir.path(), &stub_claude, &models).expect("shim written");

        let output = std::process::Command::new(&shim)
            .args(["acp", "--verbose"])
            .output()
            .expect("run shim");
        assert!(
            output.status.success(),
            "shim failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let argv = String::from_utf8(output.stdout)
            .expect("utf8 argv")
            .lines()
            .map(str::to_string)
            .collect::<Vec<_>>();

        let settings_path = argv.get(1).cloned().unwrap_or_default();
        assert_eq!(
            argv,
            vec![
                "--settings".to_string(),
                settings_path.clone(),
                "acp".to_string(),
                "--verbose".to_string(),
            ],
            "claude must receive exactly --settings <file> then the adapter's own arguments"
        );
        assert!(
            settings_path.ends_with("claude-settings.json"),
            "the settings argument must name the generated file, got {settings_path}"
        );

        let settings: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&settings_path).expect("read settings"))
                .expect("settings are valid json");
        assert_eq!(
            settings,
            serde_json::json!({
                "availableModels": ["claude-opus-4-8", "claude-sonnet-5"],
            }),
            "settings merge into the user's real config, so nothing extra may be written"
        );
    }

    #[cfg(unix)]
    #[test]
    fn shim_propagates_the_exit_status_of_claude() {
        let dir = tempfile::tempdir().expect("tempdir");
        let stub_claude = dir.path().join("claude-stub");
        write_executable(&stub_claude, "#!/bin/sh\nexit 3\n");

        let shim = write_shim(dir.path(), &stub_claude, &["claude-opus-4-8".to_string()])
            .expect("shim written");

        let status = std::process::Command::new(&shim)
            .status()
            .expect("run shim");

        assert_eq!(
            status.code(),
            Some(3),
            "a failing claude must not look healthy"
        );
    }

    #[cfg(unix)]
    #[test]
    fn shim_survives_shell_metacharacters_in_the_paths_it_wraps() {
        let dir = tempfile::tempdir().expect("tempdir");
        // `$` would expand, `"` would end a double-quoted word, and `$(id)`
        // would execute, each corrupting or breaking the generated script.
        let awkward = dir.path().join(r#"ca$h "and" $(id) dir"#);
        std::fs::create_dir_all(&awkward).expect("create awkward dir");
        let stub_claude = awkward.join("claude-stub");
        write_executable(&stub_claude, "#!/bin/sh\nprintf '%s\\n' \"$@\"\n");

        let shim = write_shim(&awkward, &stub_claude, &["claude-opus-4-8".to_string()])
            .expect("shim written");

        let output = std::process::Command::new(&shim)
            .output()
            .expect("run shim");
        assert!(
            output.status.success(),
            "shim failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let argv = String::from_utf8(output.stdout)
            .expect("utf8 argv")
            .lines()
            .map(str::to_string)
            .collect::<Vec<_>>();

        assert_eq!(
            argv,
            vec![
                "--settings".to_string(),
                awkward
                    .join("claude-settings.json")
                    .to_string_lossy()
                    .to_string(),
            ],
            "the settings path must reach claude verbatim, not shell-expanded"
        );
    }

    #[cfg(unix)]
    #[test]
    fn republishing_never_exposes_a_partial_wrapper_to_a_concurrent_exec() {
        let dir = tempfile::tempdir().expect("tempdir");
        let stub_claude = dir.path().join("claude-stub");
        write_executable(&stub_claude, "#!/bin/sh\nprintf '%s\\n' \"$@\"\n");
        let models = vec!["claude-opus-4-8".to_string()];
        let shim = write_shim(dir.path(), &stub_claude, &models).expect("first write");

        // A second session rewriting the wrapper must not make the copy an
        // already-running session is executing fail with ETXTBSY.
        for _ in 0..200 {
            write_shim(dir.path(), &stub_claude, &models).expect("concurrent rewrite");
            let output = std::process::Command::new(&shim)
                .output()
                .expect("run shim");
            assert!(
                output.status.success(),
                "wrapper became unrunnable: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

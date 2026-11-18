use std::fs::File;
use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::io::Read;
use std::io::Seek;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use anyhow::Context;
use anyhow::ensure;
use tokio::process::Command;

use super::LaunchSpec;

pub(super) fn validate_launch(spec: &LaunchSpec) -> anyhow::Result<()> {
    ensure!(
        spec.credential_refs.is_empty(),
        "unsupported Codex side-session credential references: this pin supports credential-free launches"
    );
    for key in spec.env.keys() {
        ensure!(
            matches!(
                key.as_str(),
                "PATH"
                    | "CODEX_PATH"
                    | "TMPDIR"
                    | "CODEX_SQLITE_HOME"
                    | "LANG"
                    | "LC_ALL"
                    | "TERM"
                    | "TZ"
            ),
            "unsupported Codex side-session environment input: {key}; credential scope cannot be confirmed"
        );
    }
    for name in ["config.toml", "managed_config.toml", "requirements.toml"] {
        let path = Path::new("/etc/codex").join(name);
        ensure!(
            !path.try_exists()?,
            "unsupported Codex side-session system configuration: {}; credential scope cannot be confirmed",
            path.display()
        );
    }
    Ok(())
}

/// Mount binding-owned inputs into a fresh provider home. Keep each source
/// descriptor until Bubblewrap has consumed it, including across rename.
pub(super) fn mount_inputs(
    command: &mut Command,
    binding_home: &Path,
    private_home: &Path,
) -> anyhow::Result<Vec<File>> {
    command.args(["--perms", "0700", "--dir"]).arg(private_home);
    let dir = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(binding_home.join(".codex"))
    {
        Ok(dir) => dir,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).context("opening pinned Codex home"),
    };
    let root = format!("/proc/self/fd/{}", dir.as_raw_fd());
    let mut inputs = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_str().context("unsupported Codex home entry")?;
        match name {
            "auth.json" | ".credentials.json" => anyhow::bail!(
                "unsupported Codex side-session credential file: {name}; this pin supports credential-free launches"
            ),
            "config.toml" => (),
            "skills" | "agents" | "rules" | "prompts" | "AGENTS.md" | "AGENTS.override.md" => {
                let input = OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_PATH | libc::O_NOFOLLOW)
                    .open(entry.path())?;
                let kind = input.metadata()?.file_type();
                ensure!(
                    if matches!(name, "AGENTS.md" | "AGENTS.override.md") {
                        kind.is_file()
                    } else {
                        kind.is_dir()
                    },
                    "unsupported Codex side-session input type: {name}"
                );
                command
                    .arg("--ro-bind-fd")
                    .arg(input.as_raw_fd().to_string())
                    .arg(private_home.join(name));
                inputs.push(input);
                continue;
            }
            // These belong to the parent provider process. The side starts with
            // empty state even when the parent has previously run a turn.
            "tmp"
            | ".tmp"
            | "installation_id"
            | "sessions"
            | "archived_sessions"
            | "history.jsonl"
            | "session_index.jsonl"
            | "shell_snapshots"
            | "log"
            | "models_cache.json"
            | "version.json"
            | "memories" => continue,
            database if is_runtime_database(database) => continue,
            _ => anyhow::bail!("unsupported Codex side-session home entry: {name}"),
        };
        let source = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(entry.path())
            .context("opening binding Codex config.toml")?;
        ensure!(
            source.metadata()?.is_file(),
            "unsupported Codex side-session config.toml input type"
        );
        let mut text = String::new();
        source.take(1_048_577).read_to_string(&mut text)?;
        ensure!(
            text.len() <= 1_048_576,
            "unsupported Codex side-session config.toml size"
        );
        let config: toml::Table = toml::from_str(&text)
            .map_err(|_| anyhow::anyhow!("unsupported Codex side-session config.toml syntax"))?;
        for (key, value) in &config {
            let supported = match key.as_str() {
                "model" | "model_reasoning_effort" | "sandbox_mode" | "approval_policy" => {
                    value.is_str()
                }
                "cli_auth_credentials_store" | "mcp_oauth_credentials_store" => {
                    value.as_str() == Some("file")
                }
                _ => false,
            };
            ensure!(
                supported,
                "unsupported Codex side-session configuration: {key}; credential scope cannot be confirmed"
            );
        }
        // Mount the exact config bytes we checked. A binding may edit its file
        // while a side starts.
        let mut snapshot = tempfile::tempfile()?;
        snapshot.write_all(text.as_bytes())?;
        snapshot.rewind()?;
        command
            .arg("--ro-bind-data")
            .arg(snapshot.as_raw_fd().to_string())
            .arg(private_home.join(name));
        inputs.push(snapshot);
    }
    Ok(inputs)
}

fn is_runtime_database(name: &str) -> bool {
    let name = name
        .strip_suffix("-wal")
        .or_else(|| name.strip_suffix("-shm"))
        .unwrap_or(name);
    let Some((kind, version)) = name
        .strip_suffix(".sqlite")
        .and_then(|stem| stem.rsplit_once('_'))
    else {
        return false;
    };
    matches!(kind, "state" | "goals" | "logs" | "memories" | "queue")
        && !version.is_empty()
        && version.bytes().all(|b| b.is_ascii_digit())
}

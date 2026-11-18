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

/// The parent's environment variables a side session keeps. Every other
/// variable could name a credential or redirect the provider, so the side
/// starts without it.
pub(super) const SIDE_ENV: [&str; 8] = [
    "PATH",
    "CODEX_PATH",
    "TMPDIR",
    "CODEX_SQLITE_HOME",
    "LANG",
    "LC_ALL",
    "TERM",
    "TZ",
];

pub(super) fn validate_launch(spec: &LaunchSpec) -> anyhow::Result<()> {
    ensure!(
        spec.credential_refs.is_empty() || spec.has_subscription_file(),
        "unsupported Codex side-session credential references: expected one binding subscription file"
    );
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
    subscription_file: bool,
) -> anyhow::Result<Vec<File>> {
    command.args(["--perms", "0700", "--dir"]).arg(private_home);
    let dir = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(binding_home.join(".codex"))
    {
        Ok(dir) => dir,
        Err(error) if error.kind() == ErrorKind::NotFound && !subscription_file => {
            return Ok(Vec::new());
        }
        Err(error) => return Err(error).context("opening pinned Codex home"),
    };
    let root = format!("/proc/self/fd/{}", dir.as_raw_fd());
    let mut inputs = Vec::new();
    if subscription_file {
        // Keep the directory pinned. Each auth lookup sees the publisher's current inode.
        let auth = Path::new(&root).join("auth.json");
        ensure!(
            std::fs::symlink_metadata(auth)
                .context("opening binding auth.json")?
                .is_file(),
            "unsupported Codex side-session auth.json input type"
        );
        let source = private_home.with_extension("source");
        command
            .arg("--ro-bind-fd")
            .arg(dir.as_raw_fd().to_string())
            .arg(&source)
            .arg("--symlink")
            .arg(source.join("auth.json"))
            .arg(private_home.join("auth.json"));
    }
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_str().context("unsupported Codex home entry")?;
        match name {
            "auth.json" if subscription_file => continue,
            name if subscription_file
                && name.starts_with("auth.json.")
                && name.ends_with(".tmp") =>
            {
                continue;
            }
            "auth.json" | ".credentials.json" => anyhow::bail!(
                "unsupported Codex side-session credential file: {name}; no verified source for this file"
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
            // Everything else is the parent provider's own state, such as its
            // sessions, databases, caches, plugins and lock files. The side
            // starts with an empty home and receives none of it.
            _ => continue,
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
        let mut config: toml::Table = toml::from_str(&text)
            .map_err(|_| anyhow::anyhow!("unsupported Codex side-session config.toml syntax"))?;
        // The side starts without the binding's MCP servers and their credentials.
        if config.remove("mcp_servers").is_some() {
            text = toml::to_string(&config)?;
        }
        for (key, value) in &config {
            let supported = match key.as_str() {
                "model" | "model_reasoning_effort" | "sandbox_mode" | "approval_policy" => {
                    value.is_str()
                }
                "projects" | "notice" if subscription_file => value.is_table(),
                "model_provider" if subscription_file => value.as_str() == Some("openai"),
                "openai_base_url" | "chatgpt_base_url" if subscription_file => value.is_str(),
                "forced_login_method" if subscription_file => value.as_str() == Some("chatgpt"),
                "forced_chatgpt_workspace_id" if subscription_file => {
                    value.is_str()
                        || value
                            .as_array()
                            .is_some_and(|ids| ids.iter().all(toml::Value::is_str))
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
    if subscription_file {
        inputs.push(dir);
    }
    Ok(inputs)
}

//! Comment-preserving edits for Nori's user configuration.

use crate::CONFIG_FILE;
use anyhow::Context;
use anyhow::Result;
use codex_protocol::config_types::McpServerConfig;
use codex_protocol::config_types::TrustLevel;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use toml_edit::DocumentMut;
use toml_edit::Item;
use toml_edit::Table;
use toml_edit::Value;
use toml_edit::value;

static TEMP_FILE_ID: AtomicU64 = AtomicU64::new(0);

enum ConfigEdit {
    Agent(String),
    DefaultModel { agent: String, model: String },
    ProjectTrustLevel { path: PathBuf, level: TrustLevel },
    SetPath { path: Vec<String>, value: Value },
    ClearPath { path: Vec<String> },
    ReplaceMcpServers(BTreeMap<String, McpServerConfig>),
}

/// A small builder for the config edits used by Nori's ACP frontend.
pub struct NoriConfigEdits {
    nori_home: PathBuf,
    edits: Vec<ConfigEdit>,
}

impl NoriConfigEdits {
    pub fn new(nori_home: &Path) -> Self {
        Self {
            nori_home: nori_home.to_path_buf(),
            edits: Vec::new(),
        }
    }

    pub fn set_agent(mut self, agent: &str) -> Self {
        self.edits.push(ConfigEdit::Agent(agent.to_string()));
        self
    }

    pub fn set_default_model(mut self, agent: &str, model: &str) -> Self {
        self.edits.push(ConfigEdit::DefaultModel {
            agent: agent.to_string(),
            model: model.to_string(),
        });
        self
    }

    pub fn set_project_trust_level(mut self, path: &Path, level: TrustLevel) -> Self {
        self.edits.push(ConfigEdit::ProjectTrustLevel {
            path: path.to_path_buf(),
            level,
        });
        self
    }

    /// Set a scalar value at a dotted TOML path.
    pub fn set_path<T>(mut self, path: &[&str], value: T) -> Self
    where
        T: Into<Value>,
    {
        self.edits.push(ConfigEdit::SetPath {
            path: path.iter().map(|segment| (*segment).to_string()).collect(),
            value: value.into(),
        });
        self
    }

    /// Remove a value at a dotted TOML path when it exists.
    pub fn clear_path(mut self, path: &[&str]) -> Self {
        self.edits.push(ConfigEdit::ClearPath {
            path: path.iter().map(|segment| (*segment).to_string()).collect(),
        });
        self
    }

    /// Replace the complete MCP server table with canonical protocol config.
    pub fn replace_mcp_servers(mut self, servers: &BTreeMap<String, McpServerConfig>) -> Self {
        self.edits
            .push(ConfigEdit::ReplaceMcpServers(servers.clone()));
        self
    }

    /// Apply the queued edits.
    pub async fn apply(self) -> Result<()> {
        self.apply_blocking()
    }

    /// Apply the queued edits with an atomic replacement of `config.toml`.
    pub fn apply_blocking(self) -> Result<()> {
        let config_path = self.nori_home.join(CONFIG_FILE);
        let content = match std::fs::read_to_string(&config_path) {
            Ok(content) => content,
            Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("Failed to read {}", config_path.display()));
            }
        };
        let mut document = if content.is_empty() {
            DocumentMut::new()
        } else {
            content
                .parse::<DocumentMut>()
                .with_context(|| format!("Failed to parse {}", config_path.display()))?
        };

        for edit in self.edits {
            match edit {
                ConfigEdit::Agent(agent) => document["agent"] = value(agent),
                ConfigEdit::DefaultModel { agent, model } => {
                    document["default_models"][&agent] = value(model);
                }
                ConfigEdit::ProjectTrustLevel { path, level } => {
                    let path = std::fs::canonicalize(&path).unwrap_or(path);
                    let trust_level = match level {
                        TrustLevel::Trusted => "trusted",
                        TrustLevel::Untrusted => "untrusted",
                    };
                    document["projects"][path.to_string_lossy().as_ref()]["trust_level"] =
                        value(trust_level);
                }
                ConfigEdit::SetPath { path, value } => {
                    set_path(&mut document, &path, value);
                }
                ConfigEdit::ClearPath { path } => {
                    clear_path(&mut document, &path);
                }
                ConfigEdit::ReplaceMcpServers(servers) => {
                    replace_mcp_servers(&mut document, &servers)?;
                }
            }
        }

        write_atomic(&config_path, document.to_string().as_bytes())
    }
}

fn replace_mcp_servers(
    document: &mut DocumentMut,
    servers: &BTreeMap<String, McpServerConfig>,
) -> Result<()> {
    if servers.is_empty() {
        document.as_table_mut().remove("mcp_servers");
        return Ok(());
    }

    #[derive(Serialize)]
    struct McpServers<'a> {
        mcp_servers: &'a BTreeMap<String, McpServerConfig>,
    }

    let serialized = toml::to_string(&McpServers {
        mcp_servers: servers,
    })
    .context("Failed to serialize MCP servers")?;
    let mut replacement = serialized
        .parse::<DocumentMut>()
        .context("Failed to build MCP server configuration")?;
    let mut replacement = replacement
        .as_table_mut()
        .remove("mcp_servers")
        .context("Serialized MCP configuration has no mcp_servers table")?;

    if let (Some(existing), Some(replacement)) = (
        document
            .get("mcp_servers")
            .and_then(Item::as_table)
            .map(|table| table.decor().clone()),
        replacement.as_table_mut(),
    ) {
        *replacement.decor_mut() = existing;
    }
    document.as_table_mut().insert("mcp_servers", replacement);
    Ok(())
}

fn set_path(document: &mut DocumentMut, path: &[String], mut value: Value) {
    let Some((key, parents)) = path.split_last() else {
        return;
    };
    let parent = descend(document.as_table_mut(), parents, true);
    let Some(parent) = parent else {
        return;
    };

    if let Some(existing) = parent.get(key).and_then(Item::as_value) {
        *value.decor_mut() = existing.decor().clone();
    }
    parent.insert(key, Item::Value(value));
}

fn clear_path(document: &mut DocumentMut, path: &[String]) {
    let Some((key, parents)) = path.split_last() else {
        return;
    };
    if let Some(parent) = descend(document.as_table_mut(), parents, false) {
        parent.remove(key);
    }
}

fn descend<'a>(mut table: &'a mut Table, path: &[String], create: bool) -> Option<&'a mut Table> {
    for segment in path {
        if !table.contains_key(segment) {
            if !create {
                return None;
            }
            let mut child = Table::new();
            child.set_implicit(true);
            table.insert(segment, Item::Table(child));
        }
        table = table.get_mut(segment)?.as_table_mut()?;
    }
    Some(table)
}

fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .with_context(|| format!("Failed to create {}", parent.display()))?;

    let (temp_path, mut temp_file) = loop {
        let id = TEMP_FILE_ID.fetch_add(1, Ordering::Relaxed);
        let temp_name = format!(".config.toml.{}.{id}.tmp", std::process::id());
        let temp_path = parent.join(temp_name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
        {
            Ok(file) => break (temp_path, file),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("Failed to create {}", temp_path.display()));
            }
        }
    };

    let write_result = temp_file
        .write_all(contents)
        .and_then(|()| temp_file.sync_all());
    drop(temp_file);
    if let Err(error) = write_result {
        let _ = std::fs::remove_file(&temp_path);
        return Err(error).with_context(|| format!("Failed to write {}", path.display()));
    }

    if let Err(error) = std::fs::rename(&temp_path, path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(error).with_context(|| format!("Failed to replace {}", path.display()));
    }
    Ok(())
}

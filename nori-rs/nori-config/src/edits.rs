//! Comment-preserving edits for Nori's user configuration.

use crate::CONFIG_FILE;
use crate::McpServerConfig;
use crate::NoriConfigToml;
use crate::ToolOutput;
use crate::TrustLevel;
use crate::token_providers::Protocol;
use crate::token_providers::TokenProvider;
use anyhow::Context;
use anyhow::Result;
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
    DefaultModel {
        agent: String,
        model: String,
    },
    ProjectTrustLevel {
        path: PathBuf,
        level: TrustLevel,
    },
    ProjectToolOutput {
        path: PathBuf,
        tool_output: ToolOutput,
    },
    SetPath {
        path: Vec<String>,
        value: Value,
    },
    ClearPath {
        path: Vec<String>,
    },
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

    pub fn set_project_tool_output(mut self, path: &Path, tool_output: ToolOutput) -> Self {
        self.edits.push(ConfigEdit::ProjectToolOutput {
            path: path.to_path_buf(),
            tool_output,
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

    /// Select a connection from the locked configuration and save the binding atomically.
    /// Returns the connection as declared and the protocol negotiated for this agent.
    pub fn select_token_provider(
        nori_home: &Path,
        agent: &str,
        choose: impl FnOnce(&[TokenProvider]) -> Result<Option<TokenProvider>>,
    ) -> Result<Option<(TokenProvider, Protocol)>> {
        let (_lock, mut document) = read_for_edit(nori_home)?;
        crate::legacy_pi::normalize(&mut document)?;
        let saved: NoriConfigToml = toml::from_str(&document.to_string())
            .map_err(|_| anyhow::anyhow!("could not read stored token provider configuration"))?;
        let connection = choose(&saved.token_providers)?;
        let binding_path = ["agent_token_providers".to_owned(), agent.to_owned()];
        let selected = if let Some(connection) = connection {
            connection.validate()?;
            let protocol = connection.protocol_for(agent)?;
            match saved
                .token_providers
                .iter()
                .find(|p| p.name == connection.name)
            {
                Some(existing) => anyhow::ensure!(
                    existing == &connection,
                    "token provider name is already in use"
                ),
                None => {
                    let table = toml::to_string(&connection)?
                        .parse::<DocumentMut>()?
                        .into_table();
                    let entries = document
                        .as_table_mut()
                        .entry("token_providers")
                        .or_insert_with(|| Item::ArrayOfTables(Default::default()));
                    if let Some(tables) = entries.as_array_of_tables_mut() {
                        tables.push(table);
                    } else if let Some(array) = entries.as_array_mut() {
                        array.push(Value::InlineTable(table.into_inline_table()));
                    } else {
                        anyhow::bail!("token_providers must be an array of named connections");
                    }
                }
            }
            set_path(
                &mut document,
                &binding_path,
                connection.name.as_str().into(),
            );
            Some((connection, protocol))
        } else {
            clear_path(&mut document, &binding_path);
            None
        };
        write_config(
            &nori_home.join(CONFIG_FILE),
            document.to_string().as_bytes(),
        )?;
        Ok(selected)
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

    /// Apply the queued edits with a same-directory replacement of `config.toml`.
    pub fn apply_blocking(self) -> Result<()> {
        let (_lock, mut document) = read_for_edit(&self.nori_home)?;
        for edit in self.edits {
            match edit {
                ConfigEdit::Agent(agent) => document["agent"] = value(agent),
                ConfigEdit::DefaultModel { agent, model } => {
                    document["default_models"][&agent] = value(model);
                }
                ConfigEdit::ProjectTrustLevel { path, level } => {
                    let trust_level = match level {
                        TrustLevel::Trusted => "trusted",
                        TrustLevel::Untrusted => "untrusted",
                    };
                    set_project_field(&mut document, path, "trust_level", trust_level);
                }
                ConfigEdit::ProjectToolOutput { path, tool_output } => {
                    let tool_output = match tool_output {
                        ToolOutput::Compact => "compact",
                        ToolOutput::Full => "full",
                    };
                    set_project_field(&mut document, path, "tool_output", tool_output);
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

        write_config(
            &self.nori_home.join(CONFIG_FILE),
            document.to_string().as_bytes(),
        )
    }
}

// The lock lives beside config.toml so it survives atomic replacement of the config inode.
pub(crate) fn read_for_edit(nori_home: &Path) -> Result<(std::fs::File, DocumentMut)> {
    std::fs::create_dir_all(nori_home)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(nori_home.join(".config.toml.lock"))?;
    lock.lock().context("Failed to lock Nori configuration")?;
    let config_path = nori_home.join(CONFIG_FILE);
    let content = match std::fs::read_to_string(&config_path) {
        Ok(content) => content,
        Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(error).with_context(|| format!("Failed to read {}", config_path.display()));
        }
    };
    let document = if content.is_empty() {
        DocumentMut::new()
    } else {
        content
            .parse::<DocumentMut>()
            .map_err(|error| {
                if content.contains("token_providers") {
                    anyhow::anyhow!("invalid TOML in provider configuration")
                } else {
                    error.into()
                }
            })
            .with_context(|| format!("Failed to parse {}", config_path.display()))?
    };

    Ok((lock, document))
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

/// Set one field of the `[projects]` entry keyed by the canonical project path.
fn set_project_field(document: &mut DocumentMut, path: PathBuf, field: &str, field_value: &str) {
    let key = std::fs::canonicalize(&path).unwrap_or(path);
    let project = &mut document["projects"][key.to_string_lossy().as_ref()];
    project[field] = value(field_value);
    if let Some(inline) = project.as_inline_table_mut() {
        inline.fmt();
    }
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
        let item = table.get_mut(segment)?;
        if let Some(inline) = item.as_inline_table().cloned() {
            let mut child = inline.into_table();
            child.set_implicit(true);
            *item = Item::Table(child);
        }
        table = item.as_table_mut()?;
    }
    Some(table)
}

pub(crate) fn write_config(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .with_context(|| format!("Failed to create {}", parent.display()))?;

    #[cfg(unix)]
    let target_mode = {
        use std::os::unix::fs::PermissionsExt;

        std::fs::metadata(path)
            .map(|metadata| metadata.permissions().mode())
            .unwrap_or(0o600)
    };

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

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        if let Err(error) = temp_file.set_permissions(std::fs::Permissions::from_mode(target_mode))
        {
            drop(temp_file);
            let _ = std::fs::remove_file(&temp_path);
            return Err(error).with_context(|| format!("Failed to secure {}", temp_path.display()));
        }
    }

    let write_result = temp_file
        .write_all(contents)
        .and_then(|()| temp_file.sync_all());
    drop(temp_file);
    if let Err(error) = write_result {
        let _ = std::fs::remove_file(&temp_path);
        return Err(error).with_context(|| format!("Failed to write {}", path.display()));
    }

    // `rename` replaces an existing file on Unix, but not on Windows.
    #[cfg(windows)]
    if let Err(error) = std::fs::remove_file(path)
        && error.kind() != ErrorKind::NotFound
    {
        let _ = std::fs::remove_file(&temp_path);
        return Err(error).with_context(|| format!("Failed to replace {}", path.display()));
    }

    if let Err(error) = std::fs::rename(&temp_path, path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(error).with_context(|| format!("Failed to replace {}", path.display()));
    }
    Ok(())
}

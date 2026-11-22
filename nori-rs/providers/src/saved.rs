use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use codex_keyring_store::KeyringStore;
use nori_acp_host::registry::AgentKind;
use serde::Deserialize;
use serde::Serialize;
use serde::de::DeserializeOwned;
use tracing::warn;

use crate::saved_key_env;

/// Selects where saved keys go: `auto` (keychain, else file), `keyring`, or `file`.
pub const STORE_MODE_ENV: &str = "NORI_CREDENTIALS_STORE";

const KEYRING_SERVICE: &str = "Nori Provider Keys";
const INDEX_FILE: &str = "saved-keys.json";
const SECRETS_FILE: &str = "saved-keys.secret.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreMode {
    /// The OS keychain, falling back to a 0600 file when it is unavailable.
    Auto,
    Keyring,
    File,
}

impl StoreMode {
    pub fn from_env(env: &dyn Fn(&str) -> Option<String>) -> anyhow::Result<Self> {
        match env(STORE_MODE_ENV).as_deref().map(str::trim) {
            None | Some("") | Some("auto") => Ok(StoreMode::Auto),
            Some("keyring") => Ok(StoreMode::Keyring),
            Some("file") => Ok(StoreMode::File),
            Some(other) => {
                anyhow::bail!("{STORE_MODE_ENV} must be auto, keyring, or file, not {other:?}")
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreLocation {
    Keyring,
    File,
}

/// Non-secret record of which agents have a saved key and where it lives, so
/// detection and launches can skip the keychain when nothing is saved.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SavedKeyIndex(BTreeMap<String, StoreLocation>);

impl SavedKeyIndex {
    pub fn location(&self, agent: AgentKind) -> Option<StoreLocation> {
        self.0.get(agent.slug()).copied()
    }
}

/// API keys entered through Nori, one per built-in agent.
#[derive(Debug, Clone)]
pub struct SavedKeyStore {
    nori_home: PathBuf,
    mode: StoreMode,
    keyring: Arc<dyn KeyringStore>,
}

impl SavedKeyStore {
    pub fn new(nori_home: &Path, mode: StoreMode, keyring: Arc<dyn KeyringStore>) -> Self {
        Self {
            nori_home: nori_home.to_path_buf(),
            mode,
            keyring,
        }
    }

    pub fn save(&self, agent: AgentKind, key: &str) -> anyhow::Result<StoreLocation> {
        let name = agent.display_name();
        if saved_key_env(agent).is_none() {
            anyhow::bail!("{name} has no API key variable; sign in with its own login instead");
        }
        let key = key.trim();
        if key.is_empty() || key.chars().any(char::is_control) {
            anyhow::bail!("the API key for {name} must be a single non-empty line");
        }
        let slug = agent.slug();
        let previous = self.index()?.location(agent);
        let keyring_result = match self.mode {
            StoreMode::File => None,
            StoreMode::Keyring | StoreMode::Auto => {
                Some(self.keyring.save(KEYRING_SERVICE, slug, key))
            }
        };
        let location = match (self.mode, keyring_result) {
            (_, Some(Ok(()))) => StoreLocation::Keyring,
            (StoreMode::Keyring, Some(Err(error))) => {
                anyhow::bail!("could not save the API key for {name} in the keychain: {error}")
            }
            (_, Some(Err(error))) => {
                warn!(
                    "keychain unavailable ({error}); saving the API key for {name} to {}",
                    self.secrets_path().display()
                );
                StoreLocation::File
            }
            (_, None) => StoreLocation::File,
        };
        let mut secrets: BTreeMap<String, String> = self.read_json(SECRETS_FILE)?;
        match location {
            StoreLocation::Keyring => {
                if secrets.remove(slug).is_some() {
                    self.write_json(SECRETS_FILE, &secrets)?;
                }
            }
            StoreLocation::File => {
                secrets.insert(slug.to_string(), key.to_string());
                self.write_json(SECRETS_FILE, &secrets)?;
                if previous == Some(StoreLocation::Keyring)
                    && let Err(error) = self.keyring.delete(KEYRING_SERVICE, slug)
                {
                    warn!("could not remove the old keychain entry for {name}: {error}");
                }
            }
        }
        let mut index = self.index()?;
        index.0.insert(slug.to_string(), location);
        self.write_json(INDEX_FILE, &index)?;
        Ok(location)
    }

    pub fn load(&self, agent: AgentKind) -> anyhow::Result<Option<String>> {
        let slug = agent.slug();
        match self.index()?.location(agent) {
            None => Ok(None),
            Some(StoreLocation::Keyring) => {
                self.keyring.load(KEYRING_SERVICE, slug).map_err(|error| {
                    anyhow::anyhow!(
                        "could not read the saved API key for {} from the keychain: {error}",
                        agent.display_name()
                    )
                })
            }
            Some(StoreLocation::File) => {
                let mut secrets: BTreeMap<String, String> = self.read_json(SECRETS_FILE)?;
                Ok(secrets.remove(slug))
            }
        }
    }

    pub fn delete(&self, agent: AgentKind) -> anyhow::Result<bool> {
        let slug = agent.slug();
        let mut index = self.index()?;
        let Some(location) = index.0.remove(slug) else {
            return Ok(false);
        };
        match location {
            StoreLocation::Keyring => {
                self.keyring
                    .delete(KEYRING_SERVICE, slug)
                    .map_err(|error| {
                        anyhow::anyhow!(
                            "could not remove the saved API key for {} from the keychain: {error}",
                            agent.display_name()
                        )
                    })?;
            }
            StoreLocation::File => {
                let mut secrets: BTreeMap<String, String> = self.read_json(SECRETS_FILE)?;
                secrets.remove(slug);
                self.write_json(SECRETS_FILE, &secrets)?;
            }
        }
        self.write_json(INDEX_FILE, &index)?;
        Ok(true)
    }

    pub fn index(&self) -> anyhow::Result<SavedKeyIndex> {
        self.read_json(INDEX_FILE)
    }

    fn secrets_path(&self) -> PathBuf {
        self.nori_home.join(SECRETS_FILE)
    }

    fn read_json<T: DeserializeOwned + Default>(&self, file: &str) -> anyhow::Result<T> {
        let path = self.nori_home.join(file);
        match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|_| anyhow::anyhow!("{} is not valid JSON", path.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
            Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
        }
    }

    /// Replace `file` atomically with owner-only permissions.
    fn write_json<T: Serialize>(&self, file: &str, value: &T) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.nori_home)
            .with_context(|| format!("creating {}", self.nori_home.display()))?;
        let path = self.nori_home.join(file);
        let tmp = self.nori_home.join(format!(".{file}.tmp"));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut handle = options
            .open(&tmp)
            .with_context(|| format!("writing {}", tmp.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            handle.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        handle.write_all(&serde_json::to_vec_pretty(value)?)?;
        handle.sync_all()?;
        std::fs::rename(&tmp, &path).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }
}

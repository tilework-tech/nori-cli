use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use codex_keyring_store::KeyringStore;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use crate::Provider;

/// Selects the store backend: `file` (default) or `keyring`.
pub const STORE_BACKEND_ENV: &str = "NORI_CREDENTIALS_STORE";

const FILE_NAME: &str = "credentials.json";
const KEYRING_SERVICE: &str = "Nori Credentials";
const KEYRING_ACCOUNT: &str = "credentials";
/// Index file of the earlier per-agent format, which is no longer read.
const LEGACY_INDEX: &str = "saved-keys.json";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreBackend {
    /// `$NORI_HOME/credentials.json`, owner-only.
    #[default]
    File,
    /// One OS keyring item (macOS Keychain, Secret Service on Linux).
    Keyring,
}

impl StoreBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            StoreBackend::File => "file",
            StoreBackend::Keyring => "keyring",
        }
    }

    pub fn from_env(env: &dyn Fn(&str) -> Option<String>) -> anyhow::Result<Self> {
        match env(STORE_BACKEND_ENV).as_deref().map(str::trim) {
            None | Some("") | Some("file") => Ok(StoreBackend::File),
            Some("keyring") => Ok(StoreBackend::Keyring),
            Some(other) => {
                anyhow::bail!("{STORE_BACKEND_ENV} must be file or keyring, not {other:?}")
            }
        }
    }
}

/// One saved credential. Stored as a record tagged by `type`, so other kinds
/// (such as `oauth`) can be added without changing the file format.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Credential {
    ApiKey { key: String },
}

impl Credential {
    /// An API key, trimmed; it must be a single non-empty line.
    pub fn api_key(key: &str) -> anyhow::Result<Self> {
        let key = key.trim();
        if key.is_empty() || key.chars().any(char::is_control) {
            anyhow::bail!("the API key must be a single non-empty line");
        }
        Ok(Credential::ApiKey {
            key: key.to_string(),
        })
    }

    pub fn key(&self) -> &str {
        match self {
            Credential::ApiKey { key } => key,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Credential::ApiKey { .. } => "api_key",
        }
    }
}

impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Credential({}, <redacted>)", self.type_name())
    }
}

/// Which providers have a saved record, and of what type, without secrets.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SavedIndex {
    store: StoreBackend,
    providers: BTreeMap<Provider, String>,
}

impl SavedIndex {
    pub fn store(&self) -> StoreBackend {
        self.store
    }

    /// Saved providers in [`Provider::all`] order, with each record's `type`.
    pub fn providers(&self) -> Vec<(Provider, String)> {
        self.providers
            .iter()
            .map(|(provider, kind)| (*provider, kind.clone()))
            .collect()
    }

    pub fn record_type(&self, provider: Provider) -> Option<&str> {
        self.providers.get(&provider).map(String::as_str)
    }
}

/// The saved-credential document: provider id to typed record. Records are
/// kept as JSON so types and providers this build does not know survive
/// rewrites.
type Document = BTreeMap<String, Value>;

/// Credentials entered through Nori, one record per provider.
#[derive(Debug, Clone)]
pub struct CredentialStore {
    nori_home: PathBuf,
    backend: StoreBackend,
    keyring: Arc<dyn KeyringStore>,
}

impl CredentialStore {
    pub fn new(nori_home: &Path, backend: StoreBackend, keyring: Arc<dyn KeyringStore>) -> Self {
        Self {
            nori_home: nori_home.to_path_buf(),
            backend,
            keyring,
        }
    }

    pub fn backend(&self) -> StoreBackend {
        self.backend
    }

    pub fn index(&self) -> anyhow::Result<SavedIndex> {
        let providers = self
            .read()?
            .iter()
            .filter_map(|(id, record)| {
                let provider = Provider::from_id(id)?;
                let kind = record.get("type")?.as_str()?.to_string();
                Some((provider, kind))
            })
            .collect();
        Ok(SavedIndex {
            store: self.backend,
            providers,
        })
    }

    /// The saved credential for `provider`, or `None` when nothing usable by
    /// this build is saved.
    pub fn get(&self, provider: Provider) -> anyhow::Result<Option<Credential>> {
        Ok(self.read()?.remove(provider.id()).and_then(decode))
    }

    /// Every saved credential this build can use.
    pub fn all(&self) -> anyhow::Result<BTreeMap<Provider, Credential>> {
        Ok(self
            .read()?
            .into_iter()
            .filter_map(|(id, record)| Some((Provider::from_id(&id)?, decode(record)?)))
            .collect())
    }

    pub fn set(&self, provider: Provider, credential: Credential) -> anyhow::Result<()> {
        let _lock = self.lock()?;
        let mut document = self.read()?;
        document.insert(
            provider.id().to_string(),
            serde_json::to_value(&credential)?,
        );
        self.write(&document)
    }

    /// Forget `provider`'s record. Returns whether one was saved.
    pub fn remove(&self, provider: Provider) -> anyhow::Result<bool> {
        let _lock = self.lock()?;
        let mut document = self.read()?;
        if document.remove(provider.id()).is_none() {
            return Ok(false);
        }
        self.write(&document)?;
        Ok(true)
    }

    fn file_path(&self) -> PathBuf {
        self.nori_home.join(FILE_NAME)
    }

    fn read(&self) -> anyhow::Result<Document> {
        self.reject_legacy()?;
        match self.backend {
            StoreBackend::File => {
                let path = self.file_path();
                match std::fs::read(&path) {
                    // The parse error is dropped because it can quote file contents.
                    Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| {
                        anyhow::anyhow!(
                            "{} is not a JSON object of provider records",
                            path.display()
                        )
                    }),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        Ok(Document::new())
                    }
                    Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
                }
            }
            StoreBackend::Keyring => {
                // Keyring errors are formatted with Display only: their Debug
                // form can include the stored bytes.
                let stored = self
                    .keyring
                    .load(KEYRING_SERVICE, KEYRING_ACCOUNT)
                    .map_err(|error| {
                        anyhow::anyhow!(
                            "could not read saved credentials from the OS keyring: {error}"
                        )
                    })?;
                match stored {
                    None => Ok(Document::new()),
                    Some(text) => serde_json::from_str(&text).map_err(|_| {
                        anyhow::anyhow!("the {KEYRING_SERVICE:?} OS keyring item is not valid JSON")
                    }),
                }
            }
        }
    }

    fn write(&self, document: &Document) -> anyhow::Result<()> {
        let json = serde_json::to_vec_pretty(document)?;
        match self.backend {
            StoreBackend::File => {
                // `NamedTempFile` creates the file owner-only (0600), and the
                // rename replaces the old file atomically.
                let path = self.file_path();
                let mut tmp = tempfile::NamedTempFile::new_in(&self.nori_home)
                    .with_context(|| format!("writing {}", path.display()))?;
                tmp.write_all(&json)?;
                tmp.as_file().sync_all()?;
                tmp.persist(&path)
                    .with_context(|| format!("writing {}", path.display()))?;
                Ok(())
            }
            StoreBackend::Keyring if document.is_empty() => self
                .keyring
                .delete(KEYRING_SERVICE, KEYRING_ACCOUNT)
                .map(|_| ())
                .map_err(|error| {
                    anyhow::anyhow!("could not update saved credentials in the OS keyring: {error}")
                }),
            StoreBackend::Keyring => {
                let text = String::from_utf8(json)?;
                self.keyring
                    .save(KEYRING_SERVICE, KEYRING_ACCOUNT, &text)
                    .map_err(|error| {
                        anyhow::anyhow!("could not save credentials in the OS keyring: {error}")
                    })
            }
        }
    }

    /// Serialize read-modify-write cycles across processes (the app and the
    /// CLI) with an advisory lock on `$NORI_HOME` itself, so the store needs no
    /// lock file. Released when the handle drops.
    fn lock(&self) -> anyhow::Result<std::fs::File> {
        std::fs::create_dir_all(&self.nori_home)
            .with_context(|| format!("creating {}", self.nori_home.display()))?;
        let dir = std::fs::File::open(&self.nori_home)
            .with_context(|| format!("opening {}", self.nori_home.display()))?;
        dir.lock()
            .with_context(|| format!("locking {}", self.nori_home.display()))?;
        Ok(dir)
    }

    fn reject_legacy(&self) -> anyhow::Result<()> {
        let legacy = self.nori_home.join(LEGACY_INDEX);
        if legacy.exists() {
            anyhow::bail!(
                "{} holds API keys saved per agent by an earlier development build, which this \
                 version no longer reads. Delete saved-keys.json, saved-keys.secret.json, and \
                 saved-keys.lock from {} (and any \"Nori Provider Keys\" keychain items), then \
                 save each key again with `nori providers set-key <provider>`",
                legacy.display(),
                self.nori_home.display()
            );
        }
        Ok(())
    }
}

fn decode(record: Value) -> Option<Credential> {
    serde_json::from_value(record).ok()
}

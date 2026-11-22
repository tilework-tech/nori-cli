#![allow(dead_code, clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use codex_keyring_store::CredentialStoreError;
use codex_keyring_store::KeyringStore;
use codex_keyring_store::tests::MockKeyringStore;
use nori_acp_host::registry::AgentKind;
use nori_providers::AgentAuthStatus;
use nori_providers::Credential;
use nori_providers::CredentialStore;
use nori_providers::DetectContext;
use nori_providers::KeychainProbe;
use nori_providers::Platform;
use nori_providers::Provider;
use nori_providers::SavedIndex;
use nori_providers::StoreBackend;
use nori_providers::detect_agent;

/// A keychain that only knows the listed services.
#[derive(Default)]
pub struct FakeKeychain(pub BTreeSet<String>);

impl KeychainProbe for FakeKeychain {
    fn has_generic_password(&self, service: &str) -> bool {
        self.0.contains(service)
    }
}

/// An OS keyring that is unavailable, as on a headless Linux box without Secret Service.
#[derive(Debug)]
pub struct UnavailableKeyring;

impl KeyringStore for UnavailableKeyring {
    fn load(&self, _: &str, _: &str) -> Result<Option<String>, CredentialStoreError> {
        Err(unavailable())
    }
    fn save(&self, _: &str, _: &str, _: &str) -> Result<(), CredentialStoreError> {
        Err(unavailable())
    }
    fn delete(&self, _: &str, _: &str) -> Result<bool, CredentialStoreError> {
        Err(unavailable())
    }
}

/// An OS keyring that must never be reached.
#[derive(Debug)]
pub struct ForbiddenKeyring;

impl KeyringStore for ForbiddenKeyring {
    fn load(&self, _: &str, _: &str) -> Result<Option<String>, CredentialStoreError> {
        panic!("keyring read")
    }
    fn save(&self, _: &str, _: &str, _: &str) -> Result<(), CredentialStoreError> {
        panic!("keyring write")
    }
    fn delete(&self, _: &str, _: &str) -> Result<bool, CredentialStoreError> {
        panic!("keyring delete")
    }
}

fn unavailable() -> CredentialStoreError {
    CredentialStoreError::new(keyring::Error::NoStorageAccess("no secret service".into()))
}

pub struct Fixture {
    pub dir: tempfile::TempDir,
    pub env: HashMap<String, String>,
    pub platform: Platform,
    pub keychain: FakeKeychain,
    pub saved: SavedIndex,
}

impl Fixture {
    pub fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
            env: HashMap::new(),
            platform: Platform::Linux,
            keychain: FakeKeychain::default(),
            saved: SavedIndex::default(),
        }
    }

    pub fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }

    pub fn nori_home(&self) -> PathBuf {
        self.dir.path().join("nori")
    }

    pub fn env(mut self, key: &str, value: &str) -> Self {
        self.env.insert(key.to_string(), value.to_string());
        self
    }

    pub fn on(mut self, platform: Platform) -> Self {
        self.platform = platform;
        self
    }

    pub fn keychain_item(mut self, service: &str) -> Self {
        self.keychain.0.insert(service.to_string());
        self
    }

    /// Write `contents` at `relative` under the fixture home.
    pub fn file(self, relative: &str, contents: &str) -> Self {
        let path = self.home().join(relative);
        write(&path, contents);
        self
    }

    pub fn save_key(mut self, provider: Provider, key: &str) -> Self {
        let store = file_store(&self.nori_home());
        store
            .set(provider, Credential::api_key(key).unwrap())
            .unwrap();
        self.saved = store.index().unwrap();
        self
    }

    pub fn detect(&self, agent: AgentKind) -> AgentAuthStatus {
        let home = self.home();
        let env = |key: &str| self.env.get(key).cloned();
        let ctx = DetectContext {
            home: &home,
            env: &env,
            platform: self.platform,
            keychain: &self.keychain,
            saved: &self.saved,
        };
        let status = detect_agent(agent, &ctx);
        let json = serde_json::to_string(&status).unwrap();
        assert!(!json.contains("SECRET"), "secret leaked: {json}");
        status
    }
}

pub fn write(path: &Path, contents: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

/// The default file-backed store, with a keyring that panics if touched.
pub fn file_store(nori_home: &Path) -> CredentialStore {
    CredentialStore::new(nori_home, StoreBackend::File, Arc::new(ForbiddenKeyring))
}

/// A keyring-backed store over an in-memory keyring.
pub fn keyring_store(nori_home: &Path) -> CredentialStore {
    CredentialStore::new(
        nori_home,
        StoreBackend::Keyring,
        Arc::new(MockKeyringStore::default()),
    )
}

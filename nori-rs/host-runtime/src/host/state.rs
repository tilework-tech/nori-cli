//! Private host discovery and singleton ownership. The lock inode is never removed.

use std::fs::File;
use std::fs::OpenOptions;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use serde::Deserialize;
use serde::Serialize;

#[derive(Serialize, Deserialize)]
pub struct HostState {
    pub endpoint: String,
    pub pid: i64,
}

pub struct HostFiles {
    pub directory: PathBuf,
}

impl HostFiles {
    pub fn new(home: &Path) -> anyhow::Result<Self> {
        let directory = home.join("host");
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            let mut builder = std::fs::DirBuilder::new();
            builder.mode(0o700);
            match builder.create(&directory) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
            let metadata = std::fs::symlink_metadata(&directory)?;
            anyhow::ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "host directory must not be a symlink"
            );
            validate(&metadata, &directory)?;
        }
        #[cfg(not(unix))]
        anyhow::bail!("background hosts require Unix owner-only file permissions on this platform");
        #[allow(unreachable_code)]
        Ok(Self { directory })
    }

    pub fn lock(&self) -> anyhow::Result<File> {
        private_open(&self.directory.join("lock"), true)
    }

    pub fn running(&self) -> anyhow::Result<bool> {
        let lock = self.lock()?;
        match lock.try_lock() {
            Ok(()) => Ok(false),
            Err(std::fs::TryLockError::WouldBlock) => Ok(true),
            Err(error) => Err(error.into()),
        }
    }

    pub fn read(&self) -> anyhow::Result<(HostState, String)> {
        anyhow::ensure!(
            self.running()?,
            "no host running; start one with `nori host`"
        );
        let mut state = String::new();
        private_open(&self.directory.join("state.json"), false)?.read_to_string(&mut state)?;
        let token = read_token(&self.directory.join("token"))?;
        Ok((serde_json::from_str(&state)?, token))
    }

    /// Hold this generation's state inode locked until shutdown is complete.
    /// A replacement host publishes a different inode, even at the same port.
    pub fn publish(&self, state: &HostState) -> anyhow::Result<File> {
        let temporary = self.directory.join("state.json.tmp");
        let mut file = private_open(&temporary, true)?;
        file.try_lock()?;
        file.set_len(0)?;
        file.write_all(&serde_json::to_vec(state)?)?;
        file.sync_all()?;
        std::fs::rename(&temporary, self.directory.join("state.json"))?;
        Ok(file)
    }

    /// Publish `host.json`, the single discovery file of the host contract,
    /// by writing a private temporary file and renaming it into place.
    pub fn publish_discovery(
        &self,
        discovery: &nori_protocol::HostDiscovery,
    ) -> anyhow::Result<()> {
        let temporary = self.directory.join("host.json.tmp");
        let mut file = private_open(&temporary, true)?;
        file.set_len(0)?;
        file.write_all(&serde_json::to_vec_pretty(discovery)?)?;
        file.sync_all()?;
        std::fs::rename(&temporary, self.directory.join("host.json"))?;
        Ok(())
    }

    /// The published `host.json`, read without touching the host lock;
    /// `None` when it is missing or unreadable.
    pub fn read_discovery(&self) -> Option<nori_protocol::HostDiscovery> {
        let file = private_open(&self.directory.join("host.json"), false).ok()?;
        serde_json::from_reader(file).ok()
    }

    pub fn new_token(&self) -> anyhow::Result<String> {
        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let mut file = private_open(&self.directory.join("token"), true)?;
        file.set_len(0)?;
        file.write_all(token.as_bytes())?;
        file.sync_all()?;
        Ok(token)
    }
}

pub fn read_token(path: &Path) -> anyhow::Result<String> {
    let mut token = String::new();
    private_open(path, false)?.read_to_string(&mut token)?;
    let token = token.trim().to_owned();
    anyhow::ensure!(!token.is_empty(), "host token is empty");
    Ok(token)
}

pub fn private_open(path: &Path, create: bool) -> anyhow::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(create).create(create);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .with_context(|| format!("cannot open private host file {}", path.display()))?;
    let metadata = file.metadata()?;
    anyhow::ensure!(
        metadata.is_file(),
        "host file must be a regular file: {}",
        path.display()
    );
    #[cfg(unix)]
    validate(&metadata, path)?;
    Ok(file)
}

#[cfg(unix)]
fn validate(metadata: &std::fs::Metadata, path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::fs::PermissionsExt;
    // geteuid reads the current process identity without accessing memory.
    let uid = unsafe { libc::geteuid() };
    anyhow::ensure!(
        metadata.uid() == uid && metadata.permissions().mode() & 0o077 == 0,
        "host path must be owned by this user with owner-only permissions: {}",
        path.display()
    );
    Ok(())
}

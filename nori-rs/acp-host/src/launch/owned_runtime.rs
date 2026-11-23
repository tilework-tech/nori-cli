//! Lifetime of directories created by a local runtime materializer.
use std::fs;
use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::io::Read as _;
use std::io::Write as _;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use uuid::Uuid;

const OWNER_MARKER: &[u8] = b"nori-acp-owned-runtime-v1\n";

/// A lock on behalf of the running child. Each record owns exactly one directory.
#[derive(Debug)]
pub struct OwnedRuntime {
    path: PathBuf,
    _lease: File,
}

impl OwnedRuntime {
    pub(crate) fn create(root: &Path) -> Result<Self> {
        private_dir(root)?;
        let _registry = Self::collect(root)?;
        let path = root.join(Uuid::new_v4().to_string());
        ensure!(!path.try_exists()?, "profile directory already exists");
        let mut lease = private_file()
            .create_new(true)
            .open(path.with_extension("lease"))?;
        lease.lock()?;
        lease.write_all(OWNER_MARKER)?;
        let profile = Self {
            path,
            _lease: lease,
        };
        private_dir(&profile.path)?;
        Ok(profile)
    }

    /// Collect abandoned inputs on ordinary launches without creating any files
    /// when this home has never used a materialized profile.
    pub(crate) fn sweep_existing(root: &Path) -> Result<()> {
        match fs::symlink_metadata(root) {
            Ok(metadata) => ensure!(metadata.is_dir(), "owned runtime root must be a directory"),
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err.into()),
        }
        Self::collect(root)?;
        Ok(())
    }

    fn collect(root: &Path) -> Result<File> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            ensure!(
                fs::metadata(root)?.permissions().mode() & 0o077 == 0,
                "owned runtime root must be accessible only to its owner: {}",
                root.display()
            );
        }
        // Serialize publication with collection, including crashes before mkdir.
        let registry = private_file()
            .create(true)
            .truncate(false)
            .open(root.join(".lock"))?;
        registry.lock()?;
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            let record = entry.path();
            if !entry.file_type()?.is_file() || record.extension().is_none_or(|ext| ext != "lease")
            {
                continue;
            }
            let Some(id) = record.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if Uuid::try_parse(id).is_err() {
                continue;
            }
            let mut file = match OpenOptions::new().read(true).write(true).open(&record) {
                Ok(file) => file,
                Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
                Err(err) => return Err(err.into()),
            };
            match file.try_lock() {
                Ok(()) => {
                    let mut owner = [0; OWNER_MARKER.len()];
                    if file.metadata()?.len() == OWNER_MARKER.len() as u64 {
                        file.read_exact(&mut owner)?;
                        if owner == OWNER_MARKER {
                            remove_record(&root.join(id))?;
                        }
                    }
                }
                Err(fs::TryLockError::WouldBlock) => {}
                Err(fs::TryLockError::Error(err)) => return Err(err.into()),
            }
        }

        Ok(registry)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl PartialEq for OwnedRuntime {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}

impl Eq for OwnedRuntime {}

impl Drop for OwnedRuntime {
    fn drop(&mut self) {
        if let Err(error) = remove_record(&self.path) {
            tracing::warn!("could not remove generated ACP profile: {error}");
        }
    }
}

fn remove_record(path: &Path) -> io::Result<()> {
    if let Err(error) = fs::remove_dir_all(path)
        && error.kind() != io::ErrorKind::NotFound
    {
        return Err(error);
    }
    match fs::remove_file(path.with_extension("lease")) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

pub(crate) fn private_file() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

pub(crate) fn private_dir(path: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    match builder.create(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
            ensure!(
                fs::symlink_metadata(path)?.is_dir(),
                "profile root must be a directory"
            );
            Ok(())
        }
        Err(err) => Err(err).with_context(|| format!("cannot create {}", path.display())),
    }
}

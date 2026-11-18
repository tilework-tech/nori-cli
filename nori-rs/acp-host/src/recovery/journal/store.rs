use std::fs::File;
use std::fs::OpenOptions;
use std::fs::{self};
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use anyhow::ensure;
use sha2::Digest;
use sha2::Sha256;

use super::state::Edit;
use super::state::Snapshot;

const HEADER: usize = 8 + 32;

pub(super) struct Store {
    path: PathBuf,
    state: Option<Snapshot>,
    limit: usize,
    file_bytes: usize,
    image_bound: usize,
    failed: bool,
}

impl Store {
    pub(super) fn load(path: PathBuf, binding: &str, limit: usize) -> Result<Self> {
        let mut store = Self {
            path,
            state: None,
            limit,
            file_bytes: 0,
            image_bound: 0,
            failed: false,
        };
        let mut file = match private_file().open(&store.path) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(store),
            Err(err) => return Err(err.into()),
        };
        ensure!(
            file.metadata()?.len() <= u64::try_from(limit)?,
            "journal exceeds configured limit"
        );
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let mut rest = bytes.as_slice();
        while rest.len() >= HEADER {
            let length = usize::try_from(u64::from_le_bytes(rest[..8].try_into()?))?;
            ensure!(length <= limit, "invalid journal frame size");
            if length > rest.len() - HEADER {
                break;
            }
            let body = &rest[HEADER..HEADER + length];
            ensure!(
                Sha256::digest(body)[..] == rest[8..HEADER],
                "journal checksum mismatch"
            );
            if let Some(state) = &mut store.state {
                serde_json::from_slice::<Edit>(body)?.apply(state)?;
            } else {
                let state: Snapshot = serde_json::from_slice(body)?;
                state.validate(binding)?;
                store.state = Some(state);
            }
            rest = &rest[HEADER + length..];
        }
        let state = store.state.as_ref().context("missing journal snapshot")?;
        state.validate(binding)?;
        store.image_bound = HEADER + serde_json::to_vec(state)?.len();
        store.file_bytes = bytes.len() - rest.len();
        if !rest.is_empty() {
            file.set_len(u64::try_from(store.file_bytes)?)?;
            file.sync_all()?;
        }
        Ok(store)
    }

    pub(super) fn state(&self) -> Result<Option<&Snapshot>> {
        ensure!(!self.failed, "journal persistence failure");
        Ok(self.state.as_ref())
    }

    pub(super) fn durable(&self, generation: i64) -> Result<&Snapshot> {
        let state = self.state.as_ref().context("unknown binding")?;
        ensure!(
            state.generation == generation,
            "binding generation mismatch"
        );
        Ok(state)
    }

    pub(super) fn current(&self, generation: i64) -> Result<&Snapshot> {
        ensure!(!self.failed, "journal persistence failure");
        self.durable(generation)
    }

    pub(super) fn replace(&mut self, state: Snapshot) -> Result<()> {
        let payload = serde_json::to_vec(&state)?;
        self.replace_bytes(state, &payload)
    }

    fn replace_bytes(&mut self, state: Snapshot, bytes: &[u8]) -> Result<()> {
        ensure!(bytes.len() + HEADER <= self.limit, "binding journal full");
        self.failed = true;
        let root = self.path.parent().context("missing journal directory")?;
        let temporary = root.join("writing");
        let mut file = private_file()
            .create(true)
            .truncate(true)
            .open(&temporary)?;
        write_frame(&mut file, bytes)?;
        drop(file);
        fs::rename(temporary, &self.path)?;
        sync_directory(root)?;
        self.file_bytes = bytes.len() + HEADER;
        self.image_bound = self.file_bytes;
        self.state = Some(state);
        self.failed = false;
        Ok(())
    }

    pub(super) fn edit(&mut self, change: Edit) -> Result<()> {
        let state = self.state()?.context("unknown binding")?;
        let bytes = serde_json::to_vec(&change)?;
        let frame = HEADER + bytes.len();
        // A final response also becomes a ledger result; a new stream has a header.
        let bound = self
            .image_bound
            .saturating_add(frame.saturating_mul(2))
            .saturating_add(192);
        if bound <= self.limit && self.file_bytes.saturating_add(frame) <= self.limit {
            self.failed = true;
            let mut file = private_file().append(true).open(&self.path)?;
            write_frame(&mut file, &bytes)?;
            change.apply(self.state.as_mut().context("unknown binding")?)?;
            self.file_bytes += frame;
            self.image_bound = bound;
            self.failed = false;
            return Ok(());
        }
        let output = matches!(&change, Edit::Output { .. });
        let mut compact = state.clone();
        change.apply(&mut compact)?;
        let bytes = serde_json::to_vec(&compact)?;
        // Leave compaction headroom so a stalled consumer cannot force a full rewrite per chunk.
        let ceiling = if output {
            self.limit - self.limit / 16
        } else {
            self.limit
        };
        if bytes.len() + HEADER > ceiling {
            if output && !state.blocked {
                let mut blocked = state.clone();
                blocked.blocked = true;
                self.replace(blocked)?;
            }
            bail!("binding journal full");
        }
        self.replace_bytes(compact, &bytes)
    }
}

fn write_frame(file: &mut File, bytes: &[u8]) -> Result<()> {
    file.write_all(&u64::try_from(bytes.len())?.to_le_bytes())?;
    file.write_all(&Sha256::digest(bytes))?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

pub(super) fn private_file() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    options
}

pub(super) fn sync_directory(path: &Path) -> Result<()> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x02000000);
    }
    options.open(path)?.sync_all()?;
    Ok(())
}

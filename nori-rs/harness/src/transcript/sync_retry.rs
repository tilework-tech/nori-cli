//! Durable sync retries for a managed recorder's private JSONL.
use std::io;
use std::io::SeekFrom;

use sha2::Digest;
use sha2::Sha256;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncSeekExt;
use tokio::io::AsyncWriteExt;

#[derive(Default)]
pub(super) struct SyncRetry {
    durable_end: u64,
    written_end: u64,
    pending: Sha256,
    sync_failed: bool,
}

impl SyncRetry {
    pub(super) fn appended(&mut self, bytes: &[u8]) {
        self.pending.update(bytes);
        self.written_end += bytes.len() as u64;
    }

    pub(super) fn can_retry(&self) -> bool {
        self.sync_failed
    }

    pub(super) async fn sync(&mut self, file: &mut File) -> io::Result<()> {
        let retry = std::mem::take(&mut self.sync_failed);
        file.flush().await?;
        if retry {
            self.rewrite_pending(file).await?;
        }
        if let Err(error) = file.sync_data().await {
            self.sync_failed = true;
            return Err(error);
        }
        self.durable_end = self.written_end;
        self.pending.reset();
        Ok(())
    }

    // Writeback errors may be reported only once. Dirty the unconfirmed bytes
    // again and verify them against what the writer originally appended.
    async fn rewrite_pending(&self, file: &mut File) -> io::Result<()> {
        if file.metadata().await?.len() != self.written_end {
            return Err(io::Error::other(
                "private transcript length changed after sync failure",
            ));
        }
        let mut at = file.seek(SeekFrom::Start(self.durable_end)).await?;
        let mut digest = Sha256::new();
        let mut buf = vec![0; 64 * 1024];
        while at < self.written_end {
            let n = (self.written_end - at).min(buf.len() as u64) as usize;
            let chunk = &mut buf[..n];
            file.read_exact(chunk).await?;
            digest.update(&*chunk);
            file.seek(SeekFrom::Start(at)).await?;
            file.write_all(chunk).await?;
            file.flush().await?;
            at += n as u64;
        }
        if digest.finalize() != self.pending.clone().finalize() {
            return Err(io::Error::other(
                "private transcript contents changed after sync failure",
            ));
        }
        Ok(())
    }
}

mod state;
mod store;

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fs::File;
use std::fs::{self};
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use anyhow::ensure;
use serde_json::Value;
use sha2::Digest;
use sha2::Sha256;
use uuid::Uuid;

pub use self::state::Admission;
use self::state::Command;
use self::state::Edit;
use self::state::Identity;
use self::state::Kind;
use self::state::Outcome;
pub use self::state::Record;
use self::state::Residency;
pub use self::state::Snapshot;
pub use self::state::Source;
use self::store::Store;
use self::store::private_file;
use self::store::sync_directory;

pub struct Journal {
    root: PathBuf,
    incarnation: String,
    max_binding_bytes: usize,
    bindings: Mutex<HashMap<String, Store>>,
    _lock: File,
}

impl Journal {
    pub fn open(root: &Path, max_binding_bytes: usize) -> Result<Self> {
        let mut dir = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            dir.mode(0o700);
        }
        dir.recursive(true).create(root)?;
        let lock = private_file()
            .create(true)
            .truncate(false)
            .open(root.join("journal.lock"))?;
        lock.try_lock()
            .context("managed journal already has a writer")?;
        sync_directory(root)?;
        if let Some(parent) = root.parent().filter(|path| !path.as_os_str().is_empty()) {
            sync_directory(parent)?;
        }
        Ok(Self {
            root: root.to_owned(),
            incarnation: Uuid::new_v4().to_string(),
            max_binding_bytes,
            bindings: Mutex::new(HashMap::new()),
            _lock: lock,
        })
    }

    pub fn incarnation(&self) -> &str {
        &self.incarnation
    }

    fn access<T>(&self, binding: &str, op: impl FnOnce(&mut Store) -> Result<T>) -> Result<T> {
        ensure!(!binding.is_empty(), "missing binding");
        let mut cache = self
            .bindings
            .lock()
            .map_err(|_| anyhow::anyhow!("journal lock failed"))?;
        let store = match cache.entry(binding.to_owned()) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let path = self
                    .root
                    .join(format!("{}.wal", Self::binding_file_id(binding)));
                entry.insert(Store::load(path, binding, self.max_binding_bytes)?)
            }
        };
        let result = op(store);
        if let Ok(state) = store.state()
            && state.is_none_or(|state| state.residency == Residency::Released)
        {
            cache.remove(binding);
        }
        result
    }

    pub fn register(&mut self, binding: &str, generation: i64) -> Result<()> {
        self.register_scoped(binding, generation, None)
    }

    pub fn register_identity(
        &mut self,
        binding: &str,
        generation: i64,
        conversation_id: &str,
        transcript_id: Option<&str>,
    ) -> Result<()> {
        ensure!(
            !conversation_id.is_empty() && transcript_id != Some(""),
            "invalid conversation identity"
        );
        self.register_scoped(
            binding,
            generation,
            Some(Identity {
                conversation_id: conversation_id.to_owned(),
                transcript_id: transcript_id.map(str::to_owned),
            }),
        )
    }

    fn register_scoped(
        &self,
        binding: &str,
        generation: i64,
        identity: Option<Identity>,
    ) -> Result<()> {
        ensure!(generation >= 0, "invalid generation");
        self.access(binding, |store| {
            if let Some(state) = store.state()? {
                ensure!(
                    state.generation == generation,
                    "binding generation mismatch"
                );
                if let Some(identity) = identity {
                    if let Some(saved) = &state.identity {
                        ensure!(saved == &identity, "conflicting conversation identity");
                    } else {
                        let mut state = state.clone();
                        state.identity = Some(identity);
                        store.replace(state)?;
                    }
                }
                return Ok(());
            }
            store.replace(Snapshot {
                version: 1,
                binding_id: binding.to_owned(),
                generation,
                identity,
                imported_from: None,
                launch: None,
                residency: Residency::Open,
                blocked: false,
                commands: BTreeMap::new(),
                streams: vec![],
            })
        })
    }

    pub fn contains(&self, binding: &str) -> Result<bool> {
        self.access(binding, |store| Ok(store.state()?.is_some()))
    }

    pub fn check_open(&self, binding: &str, generation: i64) -> Result<()> {
        self.access(binding, |store| {
            ensure!(
                store.current(generation)?.residency == Residency::Open,
                "binding is releasing or released"
            );
            Ok(())
        })
    }

    pub fn check(&self, binding: &str, generation: i64) -> Result<()> {
        self.access(binding, |store| {
            store.current(generation)?;
            Ok(())
        })
    }

    pub fn saved_through(&self, binding: &str, generation: i64) -> Result<Vec<Source>> {
        self.access(binding, |store| Ok(store.durable(generation)?.watermarks()))
    }

    pub fn export(&self, binding: &str, generation: i64) -> Result<Snapshot> {
        self.access(binding, |store| Ok(store.current(generation)?.clone()))
    }

    pub fn import(&mut self, binding: &str, generation: i64, snapshot: &Snapshot) -> Result<()> {
        self.import_state(binding, generation, snapshot, None)
    }

    pub fn import_checkpoint(
        &mut self,
        binding: &str,
        generation: i64,
        snapshot: &Snapshot,
        checkpoint: &Value,
    ) -> Result<()> {
        ensure!(
            serde_json::to_value(snapshot)? == checkpoint["journal"],
            "checkpoint journal mismatch"
        );
        self.import_state(
            binding,
            generation,
            snapshot,
            Some(Self::digest(checkpoint)?),
        )
    }

    pub fn digest(value: &Value) -> Result<String> {
        let mut canonical = value.clone();
        canonical.sort_all_objects();
        Ok(format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&canonical)?)
        ))
    }

    pub fn binding_file_id(binding: &str) -> String {
        format!("{:x}", Sha256::digest(binding.as_bytes()))
    }

    fn import_state(
        &self,
        binding: &str,
        generation: i64,
        snapshot: &Snapshot,
        checkpoint_digest: Option<String>,
    ) -> Result<()> {
        snapshot.validate(binding)?;
        ensure!(
            generation > snapshot.generation,
            "restore requires a newer generation"
        );
        ensure!(
            serde_json::to_vec(snapshot)?.len() <= self.max_binding_bytes,
            "checkpoint exceeds journal limit"
        );
        let journal_digest = Self::digest(&serde_json::to_value(snapshot)?)?;
        let digest = checkpoint_digest.as_ref().unwrap_or(&journal_digest);
        self.access(binding, |store| {
            let mut state = if let Some(local) = store.state()? {
                if local.generation == generation && local.imported_from.as_ref() == Some(digest) {
                    return Ok(());
                }
                if local.residency == Residency::Released {
                    let matches_receipt = local.generation == snapshot.generation
                        && checkpoint_digest.is_some()
                        && local.commands.values().any(|command| {
                            command.kind == Kind::Release
                                && command.generation == local.generation
                                && matches!(&command.outcome, Outcome::Completed(result)
                                    if result["checkpointDigest"].as_str() == Some(digest.as_str()))
                        });
                    ensure!(matches_receipt, "checkpoint differs from the final release");
                } else {
                    ensure!(
                        serde_json::to_value(local)? == serde_json::to_value(snapshot)?,
                        "restore would replace newer binding state"
                    );
                }
                local.clone()
            } else {
                snapshot.clone()
            };
            state.generation = generation;
            state.imported_from = Some(digest.clone());
            state.launch = None;
            state.residency = Residency::Open;
            store.replace(state)
        })
    }

    pub fn interrupt_prompts(&mut self, binding: &str, generation: i64) -> Result<()> {
        self.access(binding, |store| {
            if store
                .current(generation)?
                .commands
                .values()
                .any(|cmd| cmd.open_prompt(generation))
            {
                store.edit(Edit::InterruptPrompts)?;
            }
            Ok(())
        })
    }

    pub fn admit(
        &mut self,
        binding: &str,
        generation: i64,
        id: &str,
        identity: &Value,
    ) -> Result<Admission> {
        self.admit_command(binding, generation, id, identity, Kind::Command)
    }

    pub fn admit_launch(
        &mut self,
        binding: &str,
        generation: i64,
        id: &str,
        identity: &Value,
    ) -> Result<Admission> {
        self.admit_command(binding, generation, id, identity, Kind::Launch)
    }

    pub fn admit_release(
        &mut self,
        binding: &str,
        generation: i64,
        id: &str,
        identity: &Value,
    ) -> Result<Admission> {
        self.admit_command(binding, generation, id, identity, Kind::Release)
    }

    fn admit_command(
        &self,
        binding: &str,
        generation: i64,
        id: &str,
        identity: &Value,
        kind: Kind,
    ) -> Result<Admission> {
        ensure!(!id.is_empty(), "missing command ID");
        let mut identity = identity.clone();
        identity.sort_all_objects();
        let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&identity)?));
        self.access(binding, |store| {
            let state = store.current(generation)?;
            if let Some(command) = state.commands.get(id) {
                ensure!(
                    command.digest == digest && command.kind == kind,
                    "conflicting command ID"
                );
                return Ok(command.status(generation, &self.incarnation));
            }
            ensure!(
                state.residency == Residency::Open
                    || (kind == Kind::Release && state.residency == Residency::Releasing),
                "binding is releasing or released"
            );
            ensure!(
                !state.blocked || kind == Kind::Release,
                "binding journal backpressure"
            );
            match kind {
                Kind::Release => ensure!(
                    !state.commands.values().any(|cmd| cmd.kind == Kind::Release
                        && cmd.status(generation, &self.incarnation) == Admission::InFlight),
                    "release already in progress"
                ),
                Kind::Launch => ensure!(
                    state.launch.is_none(),
                    "binding generation already claimed a launch"
                ),
                Kind::Command => (),
            }
            let command = Command {
                digest,
                generation,
                host_incarnation: self.incarnation.clone(),
                kind,
                outcome: Outcome::Started,
            };
            store.edit(Edit::Start {
                id: id.to_owned(),
                command,
            })?;
            Ok(Admission::Dispatch)
        })
    }

    pub fn command(&self, binding: &str, generation: i64, id: &str) -> Result<Admission> {
        self.access(binding, |store| {
            let cmd = store
                .current(generation)?
                .commands
                .get(id)
                .context("unknown command")?;
            Ok(cmd.status(generation, &self.incarnation))
        })
    }

    pub fn complete(
        &mut self,
        binding: &str,
        generation: i64,
        id: &str,
        response: &Value,
    ) -> Result<()> {
        self.finish(binding, generation, id, response, false)
    }

    pub fn complete_release(
        &mut self,
        binding: &str,
        generation: i64,
        id: &str,
        response: &Value,
    ) -> Result<()> {
        self.finish(binding, generation, id, response, true)
    }

    fn finish(
        &self,
        binding: &str,
        generation: i64,
        id: &str,
        response: &Value,
        release: bool,
    ) -> Result<()> {
        self.access(binding, |store| {
            let command = store
                .current(generation)?
                .commands
                .get(id)
                .context("unknown command")?;
            ensure!(
                (command.kind == Kind::Release) == release,
                "command kind mismatch"
            );
            match command.status(generation, &self.incarnation) {
                Admission::Completed(saved) if saved == *response => return Ok(()),
                Admission::InFlight => (),
                Admission::Interrupted if release && command.generation == generation => (),
                _ => bail!("command no longer in flight"),
            }
            store.edit(Edit::Complete {
                id: id.to_owned(),
                result: response.clone(),
            })
        })
    }

    pub fn append(
        &mut self,
        binding: &str,
        generation: i64,
        session: &str,
        message: &Value,
    ) -> Result<Record> {
        self.record(binding, generation, None, session, message)
    }

    pub fn finish_prompt(
        &mut self,
        binding: &str,
        generation: i64,
        command: &str,
        session: &str,
        message: &Value,
    ) -> Result<Record> {
        self.record(binding, generation, Some(command), session, message)
    }

    fn record(
        &self,
        binding: &str,
        generation: i64,
        command: Option<&str>,
        session: &str,
        message: &Value,
    ) -> Result<Record> {
        ensure!(!session.is_empty(), "missing ACP session ID");
        self.access(binding, |store| {
            let state = store.current(generation)?;
            ensure!(
                state.residency != Residency::Released && !state.blocked,
                "binding released or journal blocked"
            );
            if let Some(id) = command {
                let cmd = state.commands.get(id).context("unknown command")?;
                ensure!(
                    cmd.kind == Kind::Command
                        && cmd.status(generation, &self.incarnation) == Admission::InFlight,
                    "prompt no longer in flight"
                );
            }
            let mut source = Source {
                binding_id: binding.to_owned(),
                generation,
                host_incarnation: self.incarnation.clone(),
                sequence: 1,
            };
            if let Some(stream) = state.streams.last().filter(|s| s.high.same_stream(&source)) {
                source.sequence = stream
                    .high
                    .sequence
                    .checked_add(1)
                    .context("journal sequence exhausted")?;
            }
            let record = Record {
                source,
                session_id: session.to_owned(),
                message: message.clone(),
            };
            store.edit(Edit::Output {
                record: record.clone(),
                command: command.map(str::to_owned),
            })?;
            Ok(record)
        })
    }

    pub fn replay(&self, binding: &str, generation: i64) -> Result<Vec<Record>> {
        self.access(binding, |store| {
            Ok(store
                .current(generation)?
                .streams
                .iter()
                .flat_map(|s| s.pending.iter().cloned())
                .collect())
        })
    }

    pub fn ack(&mut self, binding: &str, generation: i64, source: &Source) -> Result<()> {
        self.access(binding, |store| {
            ensure!(source.binding_id == binding, "foreign ACK");
            let stream = store
                .current(generation)?
                .streams
                .iter()
                .find(|s| s.high.same_stream(source))
                .context("unknown ACK stream")?;
            ensure!(
                (1..=stream.high.sequence).contains(&source.sequence),
                "invalid ACK sequence"
            );
            if source.sequence > stream.acknowledged {
                store.edit(Edit::Ack(source.clone()))?;
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests;

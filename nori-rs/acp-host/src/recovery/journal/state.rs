use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::collections::HashSet;
use std::collections::VecDeque;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Admission {
    Dispatch,
    InFlight,
    Interrupted,
    Completed(Value),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub binding_id: String,
    pub generation: i64,
    pub host_incarnation: String,
    pub sequence: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub source: Source,
    pub session_id: String,
    pub message: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Stream {
    pub(super) high: Source,
    pub(super) acknowledged: i64,
    pub(super) pending: VecDeque<Record>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Command {
    pub(super) digest: String,
    pub(super) generation: i64,
    pub(super) host_incarnation: String,
    pub(super) kind: Kind,
    pub(super) outcome: Outcome,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum Outcome {
    Started,
    Interrupted,
    Completed(Value),
}

impl Command {
    pub(super) fn open_prompt(&self, generation: i64) -> bool {
        self.kind == Kind::Command
            && self.generation == generation
            && matches!(self.outcome, Outcome::Started)
    }

    pub(super) fn status(&self, generation: i64, incarnation: &str) -> Admission {
        match &self.outcome {
            Outcome::Completed(value) => Admission::Completed(value.clone()),
            Outcome::Started
                if self.generation == generation && self.host_incarnation == incarnation =>
            {
                Admission::InFlight
            }
            Outcome::Started | Outcome::Interrupted => Admission::Interrupted,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) enum Residency {
    Open,
    Releasing,
    Released,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct Identity {
    pub(super) conversation_id: String,
    pub(super) transcript_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub(super) version: i64,
    pub(super) binding_id: String,
    pub(super) generation: i64,
    pub(super) identity: Option<Identity>,
    pub(super) imported_from: Option<String>,
    pub(super) launch: Option<String>,
    pub(super) residency: Residency,
    pub(super) blocked: bool,
    pub(super) commands: BTreeMap<String, Command>,
    pub(super) streams: Vec<Stream>,
}

impl Snapshot {
    pub fn conversation_id(&self) -> Option<&str> {
        self.identity
            .as_ref()
            .map(|identity| identity.conversation_id.as_str())
    }

    pub fn transcript_id(&self) -> Option<&str> {
        self.identity.as_ref()?.transcript_id.as_deref()
    }

    pub fn watermarks(&self) -> Vec<Source> {
        self.streams
            .iter()
            .map(|stream| stream.high.clone())
            .collect()
    }

    pub(super) fn validate(&self, binding: &str) -> Result<()> {
        ensure!(
            self.version == 1 && self.binding_id == binding,
            "incompatible binding journal"
        );
        ensure!(
            self.generation >= 0 && !binding.is_empty(),
            "invalid binding identity"
        );
        if let Some(identity) = &self.identity {
            ensure!(
                !identity.conversation_id.is_empty()
                    && identity.transcript_id.as_deref() != Some(""),
                "invalid conversation identity"
            );
        }
        let mut seen = HashSet::new();
        for stream in &self.streams {
            let high = &stream.high;
            ensure!(
                high.binding_id == binding
                    && (0..=self.generation).contains(&high.generation)
                    && !high.host_incarnation.is_empty()
                    && seen.insert((high.generation, &high.host_incarnation))
                    && (0..=high.sequence).contains(&stream.acknowledged)
                    && high.sequence - stream.acknowledged == i64::try_from(stream.pending.len())?,
                "invalid journal stream"
            );
            for (index, record) in stream.pending.iter().enumerate() {
                ensure!(
                    record.source.binding_id == binding
                        && record.source.generation == high.generation
                        && record.source.host_incarnation == high.host_incarnation
                        && !record.session_id.is_empty()
                        && record.source.sequence
                            == stream.acknowledged + i64::try_from(index)? + 1,
                    "invalid journal record"
                );
            }
        }
        ensure!(
            self.commands.iter().all(|(id, cmd)| !id.is_empty()
                && (0..=self.generation).contains(&cmd.generation)
                && !cmd.host_incarnation.is_empty()
                && cmd.digest.len() == 64),
            "invalid command ledger"
        );
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) enum Kind {
    Command,
    Launch,
    Release,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum Edit {
    Start {
        id: String,
        command: Command,
    },
    Complete {
        id: String,
        result: Value,
    },
    Output {
        record: Record,
        command: Option<String>,
    },
    Ack(Source),
    InterruptPrompts,
}

impl Edit {
    pub(super) fn apply(self, state: &mut Snapshot) -> Result<()> {
        match self {
            Self::Start { id, command } => {
                match command.kind {
                    Kind::Launch => state.launch = Some(id.clone()),
                    Kind::Release => state.residency = Residency::Releasing,
                    Kind::Command => (),
                }
                state.commands.insert(id, command);
            }
            Self::Complete { id, result } => {
                let command = state.commands.get_mut(&id).context("unknown command")?;
                if command.kind == Kind::Release {
                    state.residency = Residency::Released;
                }
                command.outcome = Outcome::Completed(result);
            }
            Self::Output { record, command } => {
                if let Some(id) = command {
                    state
                        .commands
                        .get_mut(&id)
                        .context("unknown command")?
                        .outcome = Outcome::Completed(record.message.clone());
                }
                let source = &record.source;
                if state
                    .streams
                    .last()
                    .is_none_or(|s| !s.high.same_stream(source))
                {
                    state.streams.push(Stream {
                        high: source.clone(),
                        acknowledged: 0,
                        pending: VecDeque::new(),
                    });
                }
                let stream = state.streams.last_mut().context("missing stream")?;
                stream.high.sequence = source.sequence;
                stream.pending.push_back(record);
            }
            Self::InterruptPrompts => {
                for cmd in state
                    .commands
                    .values_mut()
                    .filter(|cmd| cmd.open_prompt(state.generation))
                {
                    cmd.outcome = Outcome::Interrupted;
                }
            }
            Self::Ack(source) => {
                let stream = state
                    .streams
                    .iter_mut()
                    .find(|stream| stream.high.same_stream(&source))
                    .context("unknown ACK stream")?;
                let count = usize::try_from(source.sequence - stream.acknowledged)?;
                ensure!(count <= stream.pending.len(), "invalid ACK offset");
                stream.pending.drain(..count);
                stream.acknowledged = source.sequence;
                state.blocked = false;
            }
        }
        Ok(())
    }
}

impl Source {
    pub(super) fn same_stream(&self, other: &Self) -> bool {
        self.binding_id == other.binding_id
            && self.generation == other.generation
            && self.host_incarnation == other.host_incarnation
    }
}

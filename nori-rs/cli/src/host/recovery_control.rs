use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Weak;
use tokio::sync::Mutex;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use anyhow::ensure;
use nori_harness::backend::managed_additions::ManagedRecorderBarrier;
use nori_harness::managed_launch::EnsureLaunch;
use nori_harness::managed_launch::LaunchRefusal;
use nori_harness::managed_launch::ManagedLaunches;
use nori_harness::remote_agent::recovery::Checkpoint;
use nori_harness::remote_agent::recovery::Recovery;
use nori_harness::remote_agent::recovery::Scope;
use nori_harness::remote_agent::recovery::SharedRecovery;
use nori_harness::remote_agent::recovery::journal::Admission;
use nori_harness::remote_agent::recovery::journal::Source;
use nori_harness::session_manager::SessionCheckpoint;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use serde_json::json;

use super::recovery_history::History;

#[derive(Default)]
struct Binding {
    history: Mutex<Arc<History>>,
    writers: ManagedRecorderBarrier,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Export {
    #[serde(flatten)]
    checkpoint: Checkpoint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    history: Option<SessionCheckpoint>,
}

pub(super) struct RecoveryControl {
    pub shared: SharedRecovery,
    directory: PathBuf,
    bindings: Mutex<HashMap<Scope, Arc<Binding>>>,
    gates: Mutex<HashMap<String, Weak<Mutex<()>>>>,
}

impl RecoveryControl {
    pub fn open(host_dir: &Path) -> Result<Self> {
        let directory = host_dir.join("recovery");
        Ok(Self {
            shared: SharedRecovery::open(&directory)?,
            directory,
            bindings: Mutex::default(),
            gates: Mutex::default(),
        })
    }

    async fn binding(&self, scope: &Scope) -> Arc<Binding> {
        self.bindings
            .lock()
            .await
            .entry(scope.clone())
            .or_default()
            .clone()
    }

    async fn gate(&self, binding: &str) -> Arc<Mutex<()>> {
        let mut gates = self.gates.lock().await;
        if let Some(gate) = gates.get(binding).and_then(Weak::upgrade) {
            return gate;
        }
        gates.retain(|_, gate| gate.strong_count() > 0);
        let gate = Arc::new(Mutex::new(()));
        gates.insert(binding.to_owned(), Arc::downgrade(&gate));
        gate
    }

    pub async fn writers(&self, id: &str, generation: i64) -> Option<ManagedRecorderBarrier> {
        self.bindings
            .lock()
            .await
            .get(&Scope {
                binding_id: id.to_owned(),
                generation,
            })
            .map(|b| b.writers.clone())
    }

    async fn scoped<T: Send + 'static>(
        &self,
        scope: &Scope,
        f: impl FnOnce(&mut Recovery, &Scope) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let scope = scope.clone();
        self.shared.run(move |r| f(r, &scope)).await
    }

    pub async fn ensure(&self, launches: &ManagedLaunches, mut params: Value) -> Result<Value> {
        let scope =
            Scope::deserialize(&params).map_err(|e| LaunchRefusal::InvalidParams(e.into()))?;
        let gate = self.gate(&scope.binding_id).await;
        let _guard = gate.lock().await;
        let fields = params.as_object_mut().context("expected ensure params")?;
        let version = fields.remove("recoveryVersion");
        let transcript = fields
            .remove("transcriptId")
            .map(serde_json::from_value::<String>)
            .transpose()?;
        let request: EnsureLaunch = serde_json::from_value(params.clone())
            .map_err(|e| LaunchRefusal::InvalidParams(e.into()))?;
        request.validate()?;
        let known = self
            .scoped(&scope, |r, s| r.journal.contains(&s.binding_id))
            .await?;
        if version.is_none() {
            ensure!(!known, "binding requires recoveryVersion 1");
            return Ok(serde_json::to_value(launches.ensure(request)?)?);
        }
        ensure!(version == Some(json!(1)), "unsupported recovery version");
        let stable = string(&params, "conversationId")?.to_owned();
        if !known {
            ensure!(
                !launches.is_resident(&scope.binding_id)?,
                "recovery must be negotiated before launch"
            );
            launches.validate_destination(&request)?;
        }
        self.binding(&scope).await;
        self.scoped(&scope, move |r, s| {
            r.journal
                .register_identity(&s.binding_id, s.generation, &stable, transcript.as_deref())
        })
        .await?;
        self.spawn(launches, &scope, request, params).await
    }

    async fn spawn(
        &self,
        launches: &ManagedLaunches,
        scope: &Scope,
        request: EnsureLaunch,
        identity: Value,
    ) -> Result<Value> {
        let command = string(&identity, "commandId")?.to_owned();
        let id = command.clone();
        let admission = self
            .scoped(scope, move |r, s| {
                r.journal
                    .admit_launch(&s.binding_id, s.generation, &id, &identity)
            })
            .await?;
        let outcome = match admission {
            Admission::Completed(value) => value,
            Admission::Interrupted => bail!("launch interrupted; outcome unknown"),
            Admission::InFlight => bail!("launch in flight"),
            Admission::Dispatch => {
                let value = match launches.ensure(request) {
                    Ok(receipt) => json!({"result":receipt}),
                    Err(error) => {
                        let reason = match error.downcast_ref::<LaunchRefusal>() {
                            Some(LaunchRefusal::AtCapacity) => Some("atCapacity"),
                            Some(LaunchRefusal::InvalidParams(_)) => Some("invalidParams"),
                            None => None,
                        };
                        json!({"error":error.to_string(), "refusal":reason})
                    }
                };
                self.scoped(scope, move |r, s| {
                    r.journal
                        .complete(&s.binding_id, s.generation, &command, &value)?;
                    if value.get("result").is_some() {
                        r.attach(s);
                    }
                    Ok(value)
                })
                .await?
            }
        };
        if let Some(error) = outcome["error"].as_str() {
            return Err(match outcome["refusal"].as_str() {
                Some("atCapacity") => LaunchRefusal::AtCapacity.into(),
                Some("invalidParams") => {
                    LaunchRefusal::InvalidParams(anyhow::anyhow!("{error}")).into()
                }
                _ => anyhow::anyhow!("{error}"),
            });
        }
        Ok(outcome["result"].clone())
    }

    pub async fn control(
        &self,
        launches: &ManagedLaunches,
        method: &str,
        params: &Value,
    ) -> Result<Value> {
        let scope = Scope::deserialize(params)?;
        match method {
            "_nori/host/replay" => {
                self.scoped(&scope, |r, s| {
                    Ok(json!({"records":r.journal.replay(&s.binding_id,s.generation)?}))
                })
                .await
            }
            "_nori/host/ack" => {
                let source = Source::deserialize(&params["source"])?;
                self.scoped(&scope, move |r, s| {
                    r.journal.ack(&s.binding_id, s.generation, &source)?;
                    Ok(json!({}))
                })
                .await
            }
            "_nori/host/command" => {
                let id = string(params, "commandId")?.to_owned();
                self.scoped(&scope, move |r, s| {
                    Ok(match r.journal.command(&s.binding_id, s.generation, &id)? {
                        Admission::Completed(v) => json!({"status":"completed","response":v}),
                        Admission::Interrupted => json!({"status":"interrupted"}),
                        Admission::InFlight => json!({"status":"inFlight"}),
                        Admission::Dispatch => bail!("unknown command"),
                    })
                })
                .await
            }
            "_nori/host/import" | "_nori/host/release" => {
                let gate = self.gate(&scope.binding_id).await;
                let _guard = gate.lock().await;
                if method == "_nori/host/import" {
                    let binding = self.binding(&scope).await;
                    return self.import(launches, &scope, &binding, params).await;
                }
                self.scoped(&scope, |r, s| r.journal.check(&s.binding_id, s.generation))
                    .await?;
                let binding = self.binding(&scope).await;
                let output = self
                    .release(launches, &scope, &binding, string(params, "commandId")?)
                    .await?;
                self.bindings.lock().await.remove(&scope);
                Ok(output)
            }
            _ => bail!("unsupported recovery operation"),
        }
    }

    pub async fn checkpoint(
        &self,
        params: &Value,
    ) -> Result<(Value, tokio::sync::OwnedMutexGuard<()>)> {
        let scope = Scope::deserialize(params)?;
        let guard = self.gate(&scope.binding_id).await.lock_owned().await;
        self.scoped(&scope, |r, s| {
            r.journal.check_open(&s.binding_id, s.generation)
        })
        .await?;
        let binding = self.binding(&scope).await;
        let result = self.export(&scope, &binding).await;
        self.scoped(&scope, |r, s| {
            r.thaw(s);
            Ok(())
        })
        .await?;
        Ok((result?, guard))
    }

    pub async fn saved_through(&self, params: &Value) -> Option<Vec<Source>> {
        let scope = Scope::deserialize(params).ok()?;
        self.scoped(&scope, |r, s| {
            r.journal.saved_through(&s.binding_id, s.generation)
        })
        .await
        .ok()
    }

    async fn export(&self, scope: &Scope, binding: &Binding) -> Result<Value> {
        let first = self.scoped(scope, Recovery::freeze).await?;
        let writers = binding.writers.checkpoint().await;
        self.scoped(scope, move |r, s| r.recorder_barrier(s, writers))
            .await?;
        let stable = first
            .journal
            .conversation_id()
            .context("missing stable identity")?;
        let history = binding.history.lock().await.clone();
        let history = history.export(stable).await?;
        let checkpoint = self.scoped(scope, |r, s| r.checkpoint(s)).await?;
        let watermarks = checkpoint.journal.watermarks();
        ensure!(
            watermarks == first.journal.watermarks(),
            "output advanced during checkpoint"
        );
        Ok(json!({"watermarks":watermarks,"checkpoint":Export { checkpoint, history }}))
    }

    async fn import(
        &self,
        launches: &ManagedLaunches,
        scope: &Scope,
        binding: &Binding,
        params: &Value,
    ) -> Result<Value> {
        let export = Export::deserialize(&params["checkpoint"])?;
        let stable = export
            .checkpoint
            .journal
            .conversation_id()
            .context("checkpoint has no stable identity")?
            .to_owned();
        let spec = &params["launchSpec"];
        let cwd = PathBuf::from(string(spec, "cwd")?);
        let mut identity = json!({"bindingId":scope.binding_id, "generation":scope.generation,
            "commandId": format!("import:{}",scope.generation), "conversationId":stable,"spec":spec});
        let request = serde_json::from_value(identity.clone())?;
        launches.validate_destination(&request)?;
        let mode = export.checkpoint.continuation();
        identity["sourceMain"] = json!(export.checkpoint.main_session_id);
        identity["sourceLoad"] = json!(export.checkpoint.load_session);
        identity["history"] = serde_json::to_value(&export.history)?;
        let snapshot = export.checkpoint.journal;
        let restored = params["checkpoint"].clone();
        let history = Arc::new(History::default());
        history.import(&stable, export.history.as_ref(), &cwd)?;
        self.scoped(scope, move |r, s| {
            r.journal
                .import_checkpoint(&s.binding_id, s.generation, &snapshot, &restored)
        })
        .await?;
        *binding.history.lock().await = history;
        self.spawn(launches, scope, request, identity).await?;
        Ok(
            json!({"conversationId":stable, "continuation":mode, "sessionId":export.checkpoint.main_session_id}),
        )
    }

    async fn release(
        &self,
        launches: &ManagedLaunches,
        scope: &Scope,
        binding: &Binding,
        command: &str,
    ) -> Result<Value> {
        use nori_harness::remote_agent::recovery::journal::Journal;

        let id = command.to_owned();
        let admission = self
            .scoped(scope, move |r, s| {
                if r.journal.command(&s.binding_id, s.generation, &id).is_err() {
                    r.check_admission_storage(s)?;
                }
                r.journal.admit_release(
                    &s.binding_id,
                    s.generation,
                    &id,
                    &json!({"method":"release"}),
                )
            })
            .await?;
        let path = self.directory.join(format!(
            "{}.release",
            Journal::binding_file_id(&scope.binding_id)
        ));
        let prepared = self.load_release(&path, scope, command).await?;
        if let Admission::Completed(receipt) = admission {
            let saved = prepared
                .filter(|saved| saved.drained)
                .context("missing final release artifact")?;
            ensure!(
                receipt["checkpointDigest"] == Journal::digest(&saved.output["checkpoint"])?,
                "release checkpoint digest mismatch"
            );
            return Ok(saved.output);
        }
        let output = match prepared {
            Some(saved) if saved.drained => saved.output,
            earlier => {
                ensure!(
                    admission != Admission::Interrupted,
                    "release interrupted before the final producer drain; retain the previous checkpoint"
                );
                if earlier.is_none() {
                    let snapshot = self.export(scope, binding).await?;
                    save_release(
                        path.clone(),
                        ReleaseArtifact {
                            scope: scope.clone(),
                            command: command.to_owned(),
                            drained: false,
                            output: snapshot,
                        },
                    )
                    .await?;
                }
                launches
                    .release_one(&scope.binding_id, scope.generation)
                    .await?;
                self.shared.wait_for_producer(scope.clone()).await?;
                let output = self.export(scope, binding).await?;
                save_release(
                    path,
                    ReleaseArtifact {
                        scope: scope.clone(),
                        command: command.to_owned(),
                        drained: true,
                        output: output.clone(),
                    },
                )
                .await?;
                output
            }
        };
        let receipt = json!({"checkpointDigest":Journal::digest(&output["checkpoint"])?});
        let watermarks = output["watermarks"].clone();
        let id = command.to_owned();
        self.scoped(scope, move |r, s| {
            ensure!(
                serde_json::to_value(r.journal.saved_through(&s.binding_id, s.generation)?)?
                    == watermarks,
                "output advanced after the final release checkpoint"
            );
            r.journal
                .complete_release(&s.binding_id, s.generation, &id, &receipt)?;
            r.detach(s);
            Ok(())
        })
        .await?;
        Ok(output)
    }

    async fn load_release(
        &self,
        path: &Path,
        scope: &Scope,
        command: &str,
    ) -> Result<Option<ReleaseArtifact>> {
        let path = path.to_owned();
        let saved = tokio::task::spawn_blocking(move || -> Result<Option<ReleaseArtifact>> {
            let file = match super::state::private_open(&path, false) {
                Ok(file) => file,
                Err(error)
                    if error
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound) =>
                {
                    return Ok(None);
                }
                Err(error) => return Err(error),
            };
            Ok(Some(serde_json::from_reader(std::io::BufReader::new(
                file,
            ))?))
        })
        .await??;
        if let Some(saved) = &saved {
            ensure!(
                saved.scope.binding_id == scope.binding_id
                    && saved.scope.generation <= scope.generation,
                "release artifact has a conflicting scope"
            );
            if saved.scope.generation < scope.generation {
                return Ok(None);
            }
            ensure!(
                saved.command == command,
                "release artifact belongs to another command"
            );
        }
        Ok(saved)
    }
}

fn string<'a>(params: &'a Value, name: &str) -> Result<&'a str> {
    params[name]
        .as_str()
        .filter(|s| !s.is_empty())
        .with_context(|| format!("missing {name}"))
}

#[derive(Deserialize, Serialize)]
struct ReleaseArtifact {
    scope: Scope,
    command: String,
    drained: bool,
    output: Value,
}

async fn save_release(path: PathBuf, artifact: ReleaseArtifact) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        use std::io::BufWriter;
        use std::io::Write;
        let temp = path.with_extension("release-tmp");
        let file = super::state::private_open(&temp, true)?;
        file.set_len(0)?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer(&mut writer, &artifact)?;
        writer.flush()?;
        writer.get_ref().sync_all()?;
        drop(writer);
        std::fs::rename(temp, &path)?;
        std::fs::File::open(path.parent().context("missing release directory")?)?.sync_all()?;
        Ok(())
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[tokio::test]
    async fn launch_refusal_after_preflight_keeps_its_classification_on_restart() -> Result<()> {
        for fill_slot in [false, true] {
            let home = tempfile::TempDir::new()?;
            let launches = ManagedLaunches::new(home.path(), 1)?;
            let request = |binding: &str| -> Result<Value> {
                let root = home.path().join(binding);
                std::fs::create_dir_all(root.join("workspace"))?;
                std::fs::create_dir_all(root.join("home"))?;
                Ok(json!({
                    "bindingId": binding, "generation": 1, "commandId": "launch",
                    "conversationId": uuid::Uuid::new_v4(),
                    "spec": {"projectId":"fixture", "runId":"fixture", "agent":"fixture",
                        "adapter":{"path":Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/recovery/adapter.py"),"version":"1"},
                        "cwd":root.join("workspace"), "home":root.join("home"),
                        "additions":{"promptEcho":false,"statusEvents":false,"recording":false,"clientMcp":false}}
                }))
            };
            let mut params = request("candidate")?;
            launches.validate_destination(&serde_json::from_value(params.clone())?)?;
            let workspace = home.path().join("candidate/workspace");
            if fill_slot {
                launches.ensure(serde_json::from_value(request("resident")?)?)?;
            } else {
                std::fs::remove_dir(&workspace)?;
            }
            let control = RecoveryControl::open(home.path())?;
            let scope = Scope::deserialize(&params)?;
            let stable = params["conversationId"]
                .as_str()
                .context("missing fixture ID")?
                .to_owned();
            control
                .scoped(&scope, move |r, s| {
                    r.journal
                        .register_identity(&s.binding_id, s.generation, &stable, None)
                })
                .await?;
            let first = control
                .spawn(
                    &launches,
                    &scope,
                    serde_json::from_value(params.clone())?,
                    params.clone(),
                )
                .await
                .expect_err("launch must fail after preflight");
            drop(control);
            if fill_slot {
                launches.release_one("resident", 1).await?;
            } else {
                std::fs::create_dir(&workspace)?;
            }
            let restored = RecoveryControl::open(home.path())?;
            params["recoveryVersion"] = json!(1);
            let replay = restored
                .ensure(&launches, params)
                .await
                .expect_err("saved refusal must replay");
            for error in [first, replay] {
                assert!(
                    matches!(
                        (fill_slot, error.downcast_ref::<LaunchRefusal>()),
                        (true, Some(LaunchRefusal::AtCapacity))
                            | (false, Some(LaunchRefusal::InvalidParams(_)))
                    ),
                    "incorrect classification: {error}"
                );
            }
            launches.shutdown().await?;
        }
        Ok(())
    }

    #[tokio::test]
    async fn checkpoint_owns_the_binding_gate_until_its_reply_is_handed_off() -> Result<()> {
        let home = tempfile::TempDir::new()?;
        let control = RecoveryControl::open(home.path())?;
        let params = json!({"bindingId":"fixture-binding","generation":1});
        let scope = Scope::deserialize(&params)?;
        control
            .scoped(&scope, |r, s| {
                r.journal.register_identity(
                    &s.binding_id,
                    s.generation,
                    &uuid::Uuid::new_v4().to_string(),
                    None,
                )?;
                r.attach(s);
                Ok(())
            })
            .await?;
        let (_, delivery) = control.checkpoint(&params).await?;
        let lifecycle = control.gate(&scope.binding_id).await;
        assert!(lifecycle.try_lock().is_err());
        drop(delivery);
        assert!(lifecycle.try_lock().is_ok());
        Ok(())
    }
}

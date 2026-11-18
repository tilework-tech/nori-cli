use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::ensure;
use serde_json::Value;
use tokio::task;

use super::Recovery;
use super::Scope;

#[derive(Clone)]
pub struct SharedRecovery(Arc<Mutex<Recovery>>);

impl SharedRecovery {
    pub fn open(root: &Path) -> Result<Self> {
        Ok(Self(Arc::new(Mutex::new(Recovery::open(root)?))))
    }

    pub async fn run<T, F>(&self, operation: F) -> Result<T>
    where
        F: FnOnce(&mut Recovery) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let recovery = self.0.clone();
        task::spawn_blocking(move || {
            let mut recovery = recovery
                .lock()
                .map_err(|_| anyhow!("recovery state unavailable"))?;
            operation(&mut recovery)
        })
        .await?
    }

    pub async fn incoming(
        &self,
        scope: Scope,
        message: Value,
        command: Option<String>,
    ) -> Result<Option<Value>> {
        self.run(move |recovery| {
            if !recovery.journal.contains(&scope.binding_id)? {
                anyhow::ensure!(
                    command.is_none(),
                    "binding has not negotiated durable recovery"
                );
                return Ok(None);
            }
            recovery.incoming(&scope, &message, command.as_deref())
        })
        .await
    }
}

pub(super) type DrainResult = Option<std::result::Result<(), String>>;

pub(crate) struct Producer {
    scope: Scope,
    recovery: SharedRecovery,
    done: tokio::sync::watch::Sender<DrainResult>,
}

impl SharedRecovery {
    pub(crate) async fn start_producer(&self, scope: Scope) -> Result<Producer> {
        let (done, receiver) = tokio::sync::watch::channel(None);
        let selected = scope.clone();
        self.run(move |r| {
            r.journal
                .check_open(&selected.binding_id, selected.generation)?;
            let active = r.active.get_mut(&selected).context("binding is offline")?;
            ensure!(
                !active.frozen && !active.faulted,
                "binding cannot start an ACP producer"
            );
            ensure!(
                active.producer.is_none(),
                "binding already opened its ACP producer"
            );
            active.producer = Some(receiver);
            Ok(())
        })
        .await?;
        Ok(Producer {
            scope,
            recovery: self.clone(),
            done,
        })
    }

    pub async fn wait_for_producer(&self, scope: Scope) -> Result<()> {
        let receiver = self
            .run(move |r| {
                r.journal.check(&scope.binding_id, scope.generation)?;
                Ok(r.active
                    .get(&scope)
                    .and_then(|state| state.producer.clone()))
            })
            .await?;
        if let Some(mut receiver) = receiver {
            loop {
                if let Some(result) = receiver.borrow_and_update().clone() {
                    return result.map_err(anyhow::Error::msg);
                }
                receiver
                    .changed()
                    .await
                    .context("ACP producer stopped before draining output")?;
            }
        }
        Ok(())
    }
}

impl Producer {
    pub(crate) async fn finish(self, fault: Option<String>) -> Result<()> {
        let scope = self.scope;
        let state = self
            .recovery
            .run(move |r| {
                let active = r
                    .active
                    .get_mut(&scope)
                    .context("binding retired before producer drained")?;
                let fault = fault.or_else(|| {
                    (!active.pending.is_empty())
                        .then(|| "ACP producer ended with unfinished requests".to_owned())
                });
                active.faulted |= fault.is_some();
                if fault.is_some() {
                    r.journal
                        .interrupt_prompts(&scope.binding_id, scope.generation)?;
                }
                Ok(fault.map_or(Ok(()), Err))
            })
            .await;
        self.done.send_replace(Some(match &state {
            Ok(outcome) => outcome.clone(),
            Err(error) => Err(error.to_string()),
        }));
        state.map(|_| ())
    }
}

//! Manager-owned delegation admission, process authority and result delivery.
use std::collections::HashSet;
use std::sync::Weak;

use nori_protocol::Delegation;
use nori_protocol::DelegationStatus;
use nori_protocol::acp;

use super::delegation_driver::prompt;
pub(super) use super::delegation_driver::wake;
pub(super) use super::delegation_state::DelegationState;
use super::delegation_state::Entry;
use super::*;

/// Authority for one agent process to delegate through its private MCP server.
/// A weak reference prevents the server and manager from keeping each other alive.
#[derive(Clone)]
pub struct DelegationContext {
    shared: Weak<Shared>,
    process: AgentProcessId,
}

impl std::fmt::Debug for DelegationContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DelegationContext")
            .field("process", &self.process)
            .finish_non_exhaustive()
    }
}

impl DelegationContext {
    pub(super) fn new(shared: &Arc<Shared>, process: AgentProcessId) -> Self {
        Self {
            shared: Arc::downgrade(shared),
            process,
        }
    }

    pub(crate) async fn delegate(&self, agent: String, task: String) -> anyhow::Result<Delegation> {
        let shared = self
            .shared
            .upgrade()
            .ok_or_else(|| anyhow::anyhow!("session manager has closed"))?;
        let parent = {
            let state = shared.state();
            let id = state
                .registry
                .conversation_of(self.process)
                .ok_or_else(|| anyhow::anyhow!("requesting agent process has ended"))?;
            anyhow::ensure!(
                state.registry.live_process(&id) == Some(self.process),
                "requesting agent process has ended"
            );
            id
        };
        SessionManager { shared }
            .delegate_from(&parent, agent, task, Some(self.process))
            .await
    }
}

impl SessionManager {
    /// Request a child session for the user. A different agent kind waits for
    /// user approval.
    pub async fn delegate(
        &self,
        parent: &ConversationId,
        agent: String,
        task: String,
    ) -> anyhow::Result<Delegation> {
        self.delegate_from(parent, agent, task, None).await
    }

    async fn delegate_from(
        &self,
        parent: &ConversationId,
        agent: String,
        task: String,
        expected_process: Option<AgentProcessId>,
    ) -> anyhow::Result<Delegation> {
        let _admission = self.shared.admission.lock().await;
        let task = task.trim().to_string();
        anyhow::ensure!(!task.is_empty(), "delegation task must not be empty");
        let agent = crate::resolve_agent_slug(&agent);
        let value = {
            let mut state = self.shared.state();
            anyhow::ensure!(
                !state.delegation.closed.contains(parent),
                "parent session {parent} is closed"
            );
            let process = state
                .registry
                .live_process(parent)
                .ok_or_else(|| anyhow::anyhow!("parent session {parent} is not running"))?;
            anyhow::ensure!(
                expected_process.is_none_or(|expected| expected == process),
                "requesting agent process has ended"
            );
            let config = state
                .configs
                .get(&process)
                .ok_or_else(|| anyhow::anyhow!("parent session configuration is unavailable"))?;
            crate::registry::get_agent_launch_config(
                &agent,
                config.agent_token_providers.get(&agent),
            )?;
            anyhow::ensure!(
                config.multi_session,
                "delegation requires [features] multi_session = true"
            );
            let parent_agent = state
                .registry
                .record(parent)
                .ok_or(StartError::UnknownParent(*parent))?
                .agent;
            let mut depth = 1;
            let mut ancestor = Some(*parent);
            while let Some(id) = ancestor {
                let Some(record) = state.registry.record(&id) else {
                    break;
                };
                if record.source == SessionSource::Session {
                    depth += 1;
                }
                ancestor = record.parent;
            }
            anyhow::ensure!(
                depth <= 2,
                "delegation depth limit is 2 levels below the top session"
            );
            anyhow::ensure!(
                state.registry.free_slots() > 0,
                "{} sessions are already running; close one before delegating",
                state.registry.limit()
            );
            let status = if parent_agent == agent {
                DelegationStatus::Running
            } else {
                DelegationStatus::AwaitingApproval
            };
            let value = Delegation {
                request_id: ConversationId::default().to_string(),
                parent_id: parent.to_string(),
                child_id: ConversationId::default().to_string(),
                parent_agent,
                agent,
                task,
                status,
                depth,
                result: None,
                origin: if expected_process.is_some() {
                    nori_protocol::DelegationOrigin::Agent
                } else {
                    nori_protocol::DelegationOrigin::User
                },
            };
            state.delegation.entries.push(Entry {
                value: value.clone(),
                submitted: false,
                submitting: 0,
                pending_turns: HashSet::new(),
                completed_turns: HashSet::new(),
                output: String::new(),
                output_break: false,
                last_result: String::new(),
                failure: None,
                cancelled: false,
            });
            if status == DelegationStatus::AwaitingApproval {
                state.publish_delegation(&value);
            }
            value
        };
        if value.status == DelegationStatus::Running {
            self.launch_delegation(&value)?;
        }
        Ok(value)
    }

    /// Resolve exactly one pending request belonging to `parent`.
    pub async fn resolve_delegation(
        &self,
        parent: &ConversationId,
        request: &str,
        approve: bool,
    ) -> anyhow::Result<Delegation> {
        let _admission = self.shared.admission.lock().await;
        let value = {
            let mut state = self.shared.state();
            anyhow::ensure!(
                !state.delegation.closed.contains(parent),
                "parent session {parent} is closed"
            );
            anyhow::ensure!(
                state.registry.live_process(parent).is_some(),
                "parent session {parent} is not running"
            );
            let entry = state
                .delegation
                .entries
                .iter_mut()
                .find(|e| e.value.request_id == request && e.value.parent_id == parent.to_string())
                .ok_or_else(|| {
                    anyhow::anyhow!("delegation request does not belong to parent session {parent}")
                })?;
            anyhow::ensure!(
                entry.value.status == DelegationStatus::AwaitingApproval,
                "delegation request has already been resolved"
            );
            entry.value.status = if approve {
                DelegationStatus::Running
            } else {
                DelegationStatus::Declined
            };
            if !approve {
                entry.value.result = Some("The user declined this delegation.".into());
            }
            let value = entry.value.clone();
            if !approve {
                state.publish_delegation(&value);
                state.queue_result(&value);
            }
            value
        };
        if approve {
            self.launch_delegation(&value)?;
        }
        wake(&self.shared);
        Ok(value)
    }

    /// Requests made by this parent, including completed and declined requests.
    pub fn delegations(&self, parent: &ConversationId) -> Vec<Delegation> {
        let state = self.shared.state();
        state
            .delegation
            .entries
            .iter()
            .map(|entry| &entry.value)
            .chain(&state.delegation.history)
            .filter(|value| value.parent_id == parent.to_string())
            .cloned()
            .collect()
    }

    fn launch_delegation(&self, value: &Delegation) -> anyhow::Result<()> {
        let request_id = value.request_id.clone();
        let launched = self.prepare_delegation(value);
        let launched = match launched {
            Ok(launched) => launched,
            Err(error) => {
                let mut state = self.shared.state();
                if let Some(entry) = state
                    .delegation
                    .entries
                    .iter_mut()
                    .find(|entry| entry.value.request_id == value.request_id)
                {
                    entry.value.status = DelegationStatus::Failed;
                    entry.value.result = Some(error.to_string());
                    let failed = entry.value.clone();
                    state.publish_delegation(&failed);
                    state.queue_result(&failed);
                }
                drop(state);
                wake(&self.shared);
                return Err(error);
            }
        };
        {
            let mut state = self.shared.state();
            if let Some(entry) = state
                .delegation
                .entries
                .iter_mut()
                .find(|entry| entry.value.request_id == value.request_id)
            {
                entry.submitting += 1;
            }
            state.publish_delegation(value);
        }
        let manager = self.clone();
        let task = value.task.clone();
        tokio::spawn(async move {
            let result = launched.handle.prompt(prompt(&task)).await;
            manager.record_submission(Some(request_id), result, true);
        });
        Ok(())
    }

    fn prepare_delegation(&self, value: &Delegation) -> anyhow::Result<ManagedSession> {
        let parent = ConversationId::from_string(&value.parent_id)?;
        let child = ConversationId::from_string(&value.child_id)?;
        let config = {
            let state = self.shared.state();
            let process = state
                .registry
                .live_process(&parent)
                .ok_or_else(|| anyhow::anyhow!("parent session {parent} is not running"))?;
            let mut config = state
                .configs
                .get(&process)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("parent configuration unavailable"))?;
            if config.agent != value.agent {
                config.default_model =
                    config
                        .agent_token_providers
                        .get(&value.agent)
                        .map(|provider| {
                            nori_acp_host::token_provider::bound_model_id(&value.agent, provider)
                        });
            }
            config.agent.clone_from(&value.agent);
            config.conversation_id = Some(child);
            config.initial_context = None;
            config.delegation_context = None;
            config
        };
        let settings = SessionSettings {
            conversation_id: child,
            agent: value.agent.clone(),
            parent: Some(parent),
            source: SessionSource::Session,
            resumed_from: None,
            cwd: config.cwd.clone(),
            model: config.default_model.clone(),
            approval_policy: config.approval_policy,
        };
        self.launch_registered(
            SessionAgentSource::Prepare(Box::new(config.clone())),
            settings,
            config,
            SessionStart::New,
        )
        .map_err(anyhow::Error::from)
    }

    pub(super) fn record_submission(
        &self,
        request: Option<String>,
        result: anyhow::Result<acp::v1::RequestId>,
        initial: bool,
    ) {
        let accepted = result.is_ok();
        {
            let mut state = self.shared.state();
            if let Some(entry) = state
                .delegation
                .entries
                .iter_mut()
                .find(|e| request.as_ref() == Some(&e.value.request_id))
            {
                entry.submitting -= 1;
                entry.submitted |= initial;
                match result {
                    Ok(id) => {
                        if !entry.completed_turns.remove(&id) {
                            entry.pending_turns.insert(id);
                        }
                    }
                    Err(error) => {
                        if initial {
                            entry.failure = Some(error.to_string());
                        }
                    }
                }
            }
        }
        if accepted || initial {
            wake(&self.shared);
        }
    }
}

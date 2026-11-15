//! Claim completion and result work atomically, then perform I/O outside the lock.
use super::delegation_state::active;
use super::*;
use nori_protocol::DelegationStatus;
use nori_protocol::acp;

pub(super) fn prompt(text: &str) -> Vec<acp::v1::ContentBlock> {
    vec![acp::v1::ContentBlock::Text(acp::v1::TextContent::new(text))]
}

/// Claim work under the state lock, then submit prompts and stop processes outside it.
pub(super) fn wake(shared: &Arc<Shared>) {
    let manager = SessionManager {
        shared: Arc::clone(shared),
    };
    let (finished, deliveries) = {
        let mut state = shared.state();
        let mut finished = Vec::new();
        let mut changed = Vec::new();
        for index in 0..state.delegation.entries.len() {
            let entry = &state.delegation.entries[index];
            if entry.value.status != DelegationStatus::Running {
                continue;
            }
            let Ok(child) = ConversationId::from_string(&entry.value.child_id) else {
                continue;
            };
            let failed = state
                .registry
                .record(&child)
                .is_some_and(|record| record.status == SessionStatus::Failed)
                || entry.failure.is_some();
            let pending_child = state
                .delegation
                .entries
                .iter()
                .any(|e| e.value.parent_id == entry.value.child_id && active(e.value.status));
            let pending_result = state
                .delegation
                .deliveries
                .iter()
                .any(|d| d.parent == child);
            if !failed
                && (!entry.submitted
                    || entry.submitting > 0
                    || !entry.pending_turns.is_empty()
                    || pending_child
                    || pending_result)
            {
                continue;
            }
            // Registration precedes submission. A just-created child cannot complete before its task is accepted.
            if !entry.submitted && !failed {
                continue;
            }
            let entry = &mut state.delegation.entries[index];
            entry.value.status = if failed {
                DelegationStatus::Failed
            } else if entry.cancelled {
                DelegationStatus::Cancelled
            } else {
                DelegationStatus::Completed
            };
            entry.value.result = Some(if failed {
                entry
                    .failure
                    .clone()
                    .unwrap_or_else(|| "Child agent process failed".into())
            } else {
                entry.last_result.clone()
            });
            let completed = entry.value.status == DelegationStatus::Completed;
            changed.push(entry.value.clone());
            finished.push(child);
            if completed {
                state.registry.mark_task_done(&child);
            }
        }
        for value in changed {
            state.publish_delegation(&value);
            state.queue_result(&value);
        }
        let mut deliveries = Vec::new();
        for index in 0..state.delegation.deliveries.len() {
            let delivery = &state.delegation.deliveries[index];
            if let Some(previous) = delivery.process
                && state.registry.live_process(&delivery.parent) != Some(previous)
            {
                let delivery = &mut state.delegation.deliveries[index];
                delivery.in_flight = false;
                delivery.submitted = None;
                delivery.process = None;
            }
            let delivery = &state.delegation.deliveries[index];
            if delivery.in_flight || state.delegation.closed.contains(&delivery.parent) {
                continue;
            }
            let Some(process) = state.registry.live_process(&delivery.parent) else {
                continue;
            };
            let Some(handle) = state.handles.get(&process).cloned() else {
                continue;
            };
            let parent = delivery.parent;
            let owner_request = state
                .delegation
                .entries
                .iter()
                .find(|e| e.value.child_id == parent.to_string())
                .map(|e| e.value.request_id.clone());
            let delivery = &mut state.delegation.deliveries[index];
            delivery.in_flight = true;
            delivery.process = Some(process);
            deliveries.push((
                delivery.request.clone(),
                parent,
                delivery.text.clone(),
                handle,
                process,
                owner_request,
            ));
            if let Some(entry) = state
                .delegation
                .entries
                .iter_mut()
                .find(|e| e.value.child_id == parent.to_string())
            {
                entry.submitting += 1;
            }
        }
        (finished, deliveries)
    };
    for child in finished {
        let manager = manager.clone();
        tokio::spawn(async move {
            if let Err(error) = manager.close_one(&child, Duration::ZERO).await {
                tracing::warn!(%child, %error, "failed to release completed delegation process");
            }
        });
    }
    for (request, _parent, text, handle, process, owner_request) in deliveries {
        let manager = manager.clone();
        tokio::spawn(async move {
            let result = tokio::select! {
                result = handle.prompt(prompt(&text)) => result,
                () = wait_process_end(&manager.shared, process) => Err(anyhow::anyhow!("parent process ended before accepting the child result")),
            };
            {
                let mut state = manager.shared.state();
                if let Ok(id) = &result {
                    if state.delegation.completed.remove(&(process, id.clone())) {
                        state
                            .delegation
                            .deliveries
                            .retain(|d| d.request != request || d.process != Some(process));
                    } else if let Some(delivery) = state
                        .delegation
                        .deliveries
                        .iter_mut()
                        .find(|d| d.request == request && d.process == Some(process))
                    {
                        delivery.submitted = Some((process, id.clone()));
                    }
                } else if let Some(delivery) = state
                    .delegation
                    .deliveries
                    .iter_mut()
                    .find(|d| d.request == request && d.process == Some(process))
                {
                    delivery.in_flight = false;
                }
            }
            manager.record_submission(owner_request, result, false);
        });
    }
}

async fn wait_process_end(shared: &Arc<Shared>, process: AgentProcessId) {
    loop {
        let ended = shared.process_ended.notified();
        tokio::pin!(ended);
        ended.as_mut().enable();
        if !shared.state().handles.contains_key(&process) {
            return;
        }
        ended.await;
    }
}

use std::collections::HashSet;
use std::collections::VecDeque;

use codex_core::protocol::ApplyPatchApprovalRequestEvent;
use codex_core::protocol::ExecApprovalRequestEvent;
use codex_core::protocol::ExecCommandBeginEvent;
use codex_core::protocol::ExecCommandEndEvent;
use codex_core::protocol::McpToolCallBeginEvent;
use codex_core::protocol::McpToolCallEndEvent;
use codex_core::protocol::PatchApplyEndEvent;
use codex_protocol::approvals::ElicitationRequestEvent;

use super::ChatWidget;

/// Interrupts that can be queued during active streaming and flushed later.
/// Note: ExecApproval and ApplyPatchApproval are now handled immediately
/// (not deferred) to avoid deadlocks in ACP mode where the agent subprocess
/// blocks waiting for approval. They remain here for completeness.
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum QueuedInterrupt {
    ExecApproval(String, ExecApprovalRequestEvent),
    ApplyPatchApproval(String, ApplyPatchApprovalRequestEvent),
    Elicitation(ElicitationRequestEvent),
    ExecBegin(ExecCommandBeginEvent),
    ExecEnd(ExecCommandEndEvent),
    McpBegin(McpToolCallBeginEvent),
    McpEnd(McpToolCallEndEvent),
    PatchEnd(PatchApplyEndEvent),
}

#[derive(Default)]
pub(crate) struct InterruptManager {
    queue: VecDeque<QueuedInterrupt>,
}

impl InterruptManager {
    pub(crate) fn new() -> Self {
        Self {
            queue: VecDeque::new(),
        }
    }

    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Flush completion events (ExecEnd, McpEnd, PatchEnd) so in-progress
    /// tool cells transition to their finished state, then discard any
    /// remaining begin events that would create new cells below the agent's
    /// final message. Returns the number of events that were discarded.
    ///
    /// Crucially, when a Begin event is discarded, any subsequent End event
    /// with the same `call_id` is also discarded. Without this, processing
    /// an End whose Begin was discarded causes `handle_exec_end_now` to
    /// create an orphan ExecCell with the raw `call_id` as the command name
    /// (e.g. "Ran toolu_01Lt49...").
    pub(crate) fn flush_completions_and_clear(&mut self, chat: &mut ChatWidget) -> usize {
        let mut discarded = 0usize;
        let mut discarded_call_ids = HashSet::new();
        while let Some(q) = self.queue.pop_front() {
            match q {
                // Completion events: process them only if their Begin was not
                // discarded. If the Begin was discarded, the End must also be
                // discarded to avoid creating orphan cells.
                QueuedInterrupt::ExecEnd(ev) => {
                    if discarded_call_ids.contains(&ev.call_id) {
                        discarded += 1;
                    } else {
                        chat.handle_exec_end_now(ev);
                    }
                }
                QueuedInterrupt::McpEnd(ev) => {
                    if discarded_call_ids.contains(&ev.call_id) {
                        discarded += 1;
                    } else {
                        chat.handle_mcp_end_now(ev);
                    }
                }
                QueuedInterrupt::PatchEnd(ev) => chat.handle_patch_apply_end_now(ev),
                // Elicitation should not normally be queued at task completion,
                // but warn if it is.
                QueuedInterrupt::Elicitation(_) => {
                    tracing::warn!("Discarding queued elicitation request at task completion");
                    discarded += 1;
                }
                // Begin events: discard them and track their call_ids so the
                // corresponding End events are also discarded.
                QueuedInterrupt::ExecBegin(ev) => {
                    discarded_call_ids.insert(ev.call_id);
                    discarded += 1;
                }
                QueuedInterrupt::McpBegin(ev) => {
                    discarded_call_ids.insert(ev.call_id);
                    discarded += 1;
                }
                _ => {
                    discarded += 1;
                }
            }
        }
        discarded
    }

    /// Queue an exec approval request. Currently unused since approval requests
    /// are handled immediately to avoid ACP deadlocks.
    #[allow(dead_code)]
    pub(crate) fn push_exec_approval(&mut self, id: String, ev: ExecApprovalRequestEvent) {
        self.queue.push_back(QueuedInterrupt::ExecApproval(id, ev));
    }

    /// Queue a patch approval request. Currently unused since approval requests
    /// are handled immediately to avoid ACP deadlocks.
    #[allow(dead_code)]
    pub(crate) fn push_apply_patch_approval(
        &mut self,
        id: String,
        ev: ApplyPatchApprovalRequestEvent,
    ) {
        self.queue
            .push_back(QueuedInterrupt::ApplyPatchApproval(id, ev));
    }

    pub(crate) fn push_elicitation(&mut self, ev: ElicitationRequestEvent) {
        self.queue.push_back(QueuedInterrupt::Elicitation(ev));
    }

    pub(crate) fn push_exec_begin(&mut self, ev: ExecCommandBeginEvent) {
        self.queue.push_back(QueuedInterrupt::ExecBegin(ev));
    }

    pub(crate) fn push_exec_end(&mut self, ev: ExecCommandEndEvent) {
        self.queue.push_back(QueuedInterrupt::ExecEnd(ev));
    }

    pub(crate) fn push_mcp_begin(&mut self, ev: McpToolCallBeginEvent) {
        self.queue.push_back(QueuedInterrupt::McpBegin(ev));
    }

    pub(crate) fn push_mcp_end(&mut self, ev: McpToolCallEndEvent) {
        self.queue.push_back(QueuedInterrupt::McpEnd(ev));
    }

    pub(crate) fn push_patch_end(&mut self, ev: PatchApplyEndEvent) {
        self.queue.push_back(QueuedInterrupt::PatchEnd(ev));
    }
}

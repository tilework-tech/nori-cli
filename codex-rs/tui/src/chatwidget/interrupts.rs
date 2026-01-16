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
    /// Track call_ids that have deferred begin events.
    /// End events should only be deferred if their corresponding begin was deferred.
    deferred_begins: HashSet<String>,
}

impl InterruptManager {
    pub(crate) fn new() -> Self {
        Self {
            queue: VecDeque::new(),
            deferred_begins: HashSet::new(),
        }
    }

    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Check if a begin event for the given call_id was deferred.
    /// End events should only be deferred if their begin was deferred.
    #[inline]
    pub(crate) fn has_deferred_begin(&self, call_id: &str) -> bool {
        self.deferred_begins.contains(call_id)
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

    /// Queue an exec command begin event. Currently unused since begins are
    /// handled immediately for inline rendering, but kept for potential edge cases.
    #[allow(dead_code)]
    pub(crate) fn push_exec_begin(&mut self, ev: ExecCommandBeginEvent) {
        self.deferred_begins.insert(ev.call_id.clone());
        self.queue.push_back(QueuedInterrupt::ExecBegin(ev));
    }

    pub(crate) fn push_exec_end(&mut self, ev: ExecCommandEndEvent) {
        self.queue.push_back(QueuedInterrupt::ExecEnd(ev));
    }

    /// Queue an MCP tool call begin event. Currently unused since begins are
    /// handled immediately for inline rendering, but kept for potential edge cases.
    #[allow(dead_code)]
    pub(crate) fn push_mcp_begin(&mut self, ev: McpToolCallBeginEvent) {
        self.deferred_begins.insert(ev.call_id.clone());
        self.queue.push_back(QueuedInterrupt::McpBegin(ev));
    }

    pub(crate) fn push_mcp_end(&mut self, ev: McpToolCallEndEvent) {
        self.queue.push_back(QueuedInterrupt::McpEnd(ev));
    }

    pub(crate) fn push_patch_end(&mut self, ev: PatchApplyEndEvent) {
        self.queue.push_back(QueuedInterrupt::PatchEnd(ev));
    }

    pub(crate) fn flush_all(&mut self, chat: &mut ChatWidget) {
        while let Some(q) = self.queue.pop_front() {
            match q {
                QueuedInterrupt::ExecApproval(id, ev) => chat.handle_exec_approval_now(id, ev),
                QueuedInterrupt::ApplyPatchApproval(id, ev) => {
                    chat.handle_apply_patch_approval_now(id, ev)
                }
                QueuedInterrupt::Elicitation(ev) => chat.handle_elicitation_request_now(ev),
                QueuedInterrupt::ExecBegin(ev) => chat.handle_exec_begin_now(ev),
                QueuedInterrupt::ExecEnd(ref ev) => {
                    self.deferred_begins.remove(&ev.call_id);
                    chat.handle_exec_end_now(ev.clone());
                }
                QueuedInterrupt::McpBegin(ev) => chat.handle_mcp_begin_now(ev),
                QueuedInterrupt::McpEnd(ref ev) => {
                    self.deferred_begins.remove(&ev.call_id);
                    chat.handle_mcp_end_now(ev.clone());
                }
                QueuedInterrupt::PatchEnd(ev) => chat.handle_patch_apply_end_now(ev),
            }
        }
    }
}

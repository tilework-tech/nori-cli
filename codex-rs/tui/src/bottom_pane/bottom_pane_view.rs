use std::collections::HashMap;

use crate::bottom_pane::ApprovalRequest;
use crate::render::renderable::Renderable;
use codex_protocol::protocol::McpAuthStatus;
use crossterm::event::KeyEvent;

use super::CancellationEvent;

/// Trait implemented by every view that can be shown in the bottom pane.
pub(crate) trait BottomPaneView: Renderable {
    /// Handle a key event while the view is active. A redraw is always
    /// scheduled after this call.
    fn handle_key_event(&mut self, _key_event: KeyEvent) {}

    /// Return `true` if the view has finished and should be removed.
    fn is_complete(&self) -> bool {
        false
    }

    /// Handle Ctrl-C while this view is active.
    fn on_ctrl_c(&mut self) -> CancellationEvent {
        CancellationEvent::NotHandled
    }

    /// Optional paste handler. Return true if the view modified its state and
    /// needs a redraw.
    fn handle_paste(&mut self, _pasted: String) -> bool {
        false
    }

    /// Try to handle approval request; return the original value if not
    /// consumed.
    fn try_consume_approval_request(
        &mut self,
        request: ApprovalRequest,
    ) -> Option<ApprovalRequest> {
        Some(request)
    }

    /// Update MCP server auth statuses. Only meaningful for the MCP server
    /// picker; other views ignore this.
    fn update_mcp_auth_statuses(&mut self, _statuses: &HashMap<String, McpAuthStatus>) {}

    /// Handle MCP OAuth login completion. Only meaningful for the MCP server
    /// picker; other views ignore this.
    fn handle_mcp_oauth_complete(&mut self, _server_name: &str, _success: bool) {}
}

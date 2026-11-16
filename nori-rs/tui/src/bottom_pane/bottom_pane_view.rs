use std::collections::HashMap;

use crate::bottom_pane::ApprovalRequest;
use crate::render::renderable::Renderable;
use codex_rmcp_client::McpAuthStatus;
use crossterm::event::KeyEvent;

use super::CancellationEvent;

/// Trait implemented by every view that can be shown in the bottom pane.
pub(crate) trait BottomPaneView: Renderable {
    /// Handle a key event while the view is active. A redraw is always
    /// scheduled after this call.
    fn handle_key_event(&mut self, _key_event: KeyEvent) {}

    /// Return `true` while the view needs every key, including App-level
    /// hotkeys, for example while it records a new key binding.
    fn captures_all_keys(&self) -> bool {
        false
    }

    /// Return `true` if the view has finished and should be removed.
    fn is_complete(&self) -> bool {
        false
    }

    /// Handle Ctrl-C while this view is active.
    fn on_ctrl_c(&mut self) -> CancellationEvent {
        CancellationEvent::NotHandled
    }

    /// Handle Escape while this view is active. Views with multi-stage Escape
    /// behavior can override this independently from Ctrl-C cancellation.
    fn on_escape(&mut self) -> CancellationEvent {
        self.on_ctrl_c()
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

    /// Stop asking for a request that was answered elsewhere. Returns true
    /// when this view held it.
    fn dismiss_request(&mut self, _request_id: &nori_protocol::acp::v1::RequestId) -> bool {
        false
    }

    /// Update MCP server auth statuses. Only meaningful for the MCP server
    /// picker; other views ignore this.
    fn update_mcp_auth_statuses(&mut self, _statuses: &HashMap<String, McpAuthStatus>) {}

    /// Handle MCP OAuth login completion. Only meaningful for the MCP server
    /// picker; other views ignore this.
    fn handle_mcp_oauth_complete(&mut self, _server_name: &str, _success: bool) {}

    /// Update an item in a generic selection view by stable id.
    fn update_selection_item(
        &mut self,
        _stable_id: &str,
        _name: String,
        _description: Option<String>,
        _search_value: String,
    ) -> bool {
        false
    }

    /// Replace the rows of the session list. Only the session list uses this;
    /// other views ignore it.
    fn update_session_list(&mut self, _rows: &[super::SessionRow]) -> bool {
        false
    }

    fn is_session_list(&self) -> bool {
        false
    }

    /// Whether a selection view owns this stable action identifier.
    fn has_selection_item(&self, _stable_id: &str) -> bool {
        false
    }

    /// Remove an item in a generic selection view by stable id.
    fn remove_selection_item(&mut self, _stable_id: &str) -> bool {
        false
    }
}

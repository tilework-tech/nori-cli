# ACP Integration Design Summary

- `codex-acp` is a parallel crate to `codex-core`, not integrated via shared traits
- Minimal modifications to `codex-core` to ease upstream merge burden
- ACP vs HTTP mode is determined at startup via config, no mid-session switching
- TUI/CLI branches once at startup: `if config.acp_agent.is_some() { run_acp_mode() } else { run_http_mode() }`
- HTTP mode code path remains completely unchanged
- ACP agents fully own their conversation history; Codex does not persist or convert it
- Placeholder comments mark future integration points for history persistence, export, and resume/fork
- Approval bridging is the single integration point between ACP and Codex UI
- `ApprovalRequest` bundles `ExecApprovalRequestEvent`, ACP options, and a oneshot response channel
- TUI receives approval requests via `AcpConnection::take_approval_receiver()`
- TUI sends user decision back via the oneshot channel as `ReviewDecision`
- `translator.rs` handles bi-directional type conversion between ACP and Codex formats
- `permission_request_to_approval_event()` converts ACP requests to Codex format
- `review_decision_to_permission_outcome()` converts Codex decisions back to ACP format
- Fallback behavior: auto-approve if approval channel closed, deny if response channel dropped

# PR #738 at 8ea6650e: pass

The delta from e3829922 is test-only: `attached_external_cancel.rs` sets the accepted stream blocking before the WebSocket handshake.

- Full matrix at this exact binary (built from 8ea6650e, 2026-11-24): 146/179, every check identical to main `f9eedcac` (146/179). No regression.
  The new S16-S20 scenarios (watcher reconnect, silent loss, thought indicator, output cap, agent-started cancel) were repeated twice more with the same result.
- Linux CI: green (run 45311658296).
- Native macOS, exact SHA (run 45311654214): host_lifecycle and managed_recovery pass. The attach, host client and attached_external_cancel tests pass. The full suite fails only #737's `requests_piped_before_stdin_closes_are_all_answered_by_the_joined_host`.
- Local Linux: attached_external_cancel passed 5 of 5 repeated runs.

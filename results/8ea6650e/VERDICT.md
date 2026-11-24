# PR #738 at 8ea6650e: pass

The delta from e3829922 is test-only: `attached_external_cancel.rs` sets the accepted stream blocking before the WebSocket handshake.
The nori binary is byte-identical to e3829922 and 7b95d4b5, so the e3829922 matrix (79/107, same as main) applies.

- Linux CI: green (run 45311658296).
- Native macOS, exact SHA (run 45311654214): host_lifecycle and managed_recovery pass. The attach, host client and attached_external_cancel tests pass. The full suite fails only #737's `requests_piped_before_stdin_closes_are_all_answered_by_the_joined_host`.
- Local Linux: attached_external_cancel passed 5 of 5 repeated runs.

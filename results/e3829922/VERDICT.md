# PR #738 at e3829922 — pass

refactor(tui): extract the host session client from attach

- Matrix: 79/107, identical per check to main f9eedcac (no check changed state). Every failure is a later-layer gap listed in MATRIX.md.
- Local Linux: tui-pty-e2e host_attach 14/14, attached_external_cancel 2/2; nori-cli host_lifecycle 120 (1 ignored), managed_recovery 23; nori-tui host_client 15, attach 100.
- Native macOS (exact SHA, run 45310393308): host_lifecycle + managed_recovery pass; attached_external_cancel `ctrl_c_cancels_once_and_preserves_the_attached_draft` failed with HandshakeError::Interrupted (pre-existing test race, see below); full suite fails only #737's `requests_piped_before_stdin_closes_are_all_answered_by_the_joined_host`.
- macOS at 7b95d4b5 (run 45310058952, same nori binary): all three targeted steps pass.

Pre-existing flake (not this PR): `attached_external_cancel.rs:169` calls `tungstenite::accept(stream)` on a stream accepted from a nonblocking listener. On macOS/BSD the accepted socket inherits O_NONBLOCK (Linux does not), so the handshake read can return WouldBlock -> Interrupted. Set `stream.set_nonblocking(false)` before `accept`.

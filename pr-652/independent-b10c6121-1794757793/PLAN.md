# D2 independent verification plan
Commit b10c6121eb30a9cd2be7a45e589e6473f31a86a8; no product edits or merge.
Artifacts: .local/pr652; durable output: one PR #652 comment; PNGs on nori/pr-proofs.
Build once in cli using cargo --manifest-path nori-rs/Cargo.toml; CARGO_HOME/RUSTUP_HOME under /home/sprite. Jobs=2. No shared target.
Run nori-harness, nori-acp-host, nori-tui, nori-cli, tui-pty-e2e --no-fail-fast, RUST_TEST_THREADS=2; set MOCK_ACP_AGENT_BIN. No suite-wide NORI_NO_ANALYTICS or mock instruction env.

Capture plan: all sessions 120x40; root owns cleanup using tui-stop.
- d2-shell: host start/duplicate/status/stop/stale/occupied/auth results; assert exact terminal output.
- d2-a: plain attach list; /new; prompt; assert response; Ctrl+D detach then named attach history.
- d2-b/d2-c: named attach; assert WATCHING; submit rejected prompt; Alt+T; assert CONTROLLING/former controller notice.
- pending permission: send mock:request-permission:echo D2 approval; B Enter rejected; Alt+T then Enter allowed.
- midturn: mock:stream-until-cancel; takeover from C; cancel; verify all clients receive result.
- d2-network: named attach via local proxy; close TCP then blackhole sockets; assert RECONNECTING; generate unique missed turns; restore; assert exact-once replay.
- crashed session: exact prompt mock:exit-during-prompt; Alt+S; Ctrl+R; verify same ID/order/history, subscribed watcher sees next prompt.
- host restart: kill own host then restart same port; bounded observe old client and fresh attach; record clear failure vs endless reconnect.
- d2-single: multi_session=false regular prompt/answer flow.
Capture using capture-session, retain raw delimiter ANSI and strip final delimiter before render-ansi re-render. Inspect PNGs and screen text. Clean up only test-owned processes.

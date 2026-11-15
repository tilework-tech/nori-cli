# Independent PR651 verification runs

Pinned source: 58d261cbc421d927b77c2af146a7d83b748af628.
All product source remained unchanged. Rust 1.90.0, dedicated CARGO_TARGET_DIR, 2 build jobs, 2 test threads.

Requested complete run:

```sh
cargo test -p nori-harness -p nori-tui -p tui-pty-e2e --no-fail-fast -- --test-threads=2
```

The full log includes every target and doc-test through the final summary. Initial totals:
- nori-harness: 423 passed, 0 failed, 1 ignored.
- nori-tui: 1450 passed, 1 failed, 4 ignored.
- tui-pty-e2e: 182 passed, 2 failed, 9 ignored.

The three failures were caused by the independent tester exporting fixture-only variables to the entire Cargo run:
- NORI_MOCK_INSTRUCTION_FILES=1 replaced the real instruction inventory expected by local_context_command_returns_promptly_and_loads_instruction_inventory.
- NORI_NO_ANALYTICS=1 prevented the two analytics tests from calling their local fake analytics endpoint.

No product changes were made to resolve these. Both variables were unset; full nori-tui rerun passed 1451 tests/4 ignored, and both PTY analytics tests passed. Other PTY tests control instruction mocking per child fixture themselves.

An intermediate direct unit-binary run also lacked corrected Modal Rust environment variables, causing Insta's cargo metadata lookup to fail and select incorrect snapshot paths. Its generated .snap.new files were removed; the subsequent direct and Cargo runs with CARGO_HOME/RUSTUP_HOME corrected passed. Intermediate diagnostic logs are included with explicit names.

The full command's enclosing shell ended with 143 after Cargo printed its complete final summary and before the planned supplemental command. Supplemental checks were run separately and their exit codes recorded.

Final requested-suite outcomes after clean environment reruns: 423 harness + 1451 TUI + 184 PTY = 2058 passed; 14 pre-existing ignored tests. Known cloud auth-error test passed this time.

Supplemental commands:

```sh
cargo test -p nori-tui --no-fail-fast -- --test-threads=2
cargo test -p tui-pty-e2e --test analytics --no-fail-fast -- --test-threads=2
cargo test -p nori-cli --test headless_exec --no-fail-fast -- --test-threads=2
cargo test -p nori-exec --no-fail-fast -- --test-threads=2
cargo test -p nori-acp-host --test remote_ws --no-fail-fast -- --test-threads=2
```

Additional behavioral probes and their raw evidence live in the adjacent wire/PTY archives. Initial probe fixture mistakes were corrected and rerun; diagnostic attempts remain labelled in the PTY archive. Loop tests use tui.loop_count, since /loop is not a built-in command. Cloud UI probes use a mock Handroll adapter, not a real broker.

# Exact-head test execution

Commit e9ff6232c3269111f720128dcde16389827b1abb. Linux Modal runtime. Rust 1.90.0. Each command ran from nori-rs with jobs 2, debug info disabled, and test threads 2. No test/product source changes or snapshot updates.

```sh
export CARGO_HOME="$HOME/.cargo" RUSTUP_HOME="$HOME/.rustup"
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
cargo build --bin nori --bin mock_acp_agent
export MOCK_ACP_AGENT_BIN="$PWD/target/debug/mock_acp_agent"
unset NORI_MOCK_INSTRUCTION_FILES NORI_NO_ANALYTICS
cargo test -p nori-harness --no-fail-fast -- --test-threads=2
cargo test -p nori-tui --no-fail-fast -- --test-threads=2
cargo test -p tui-pty-e2e --no-fail-fast -- --test-threads=2
```

The suite wrapper exited 143 during PTY execution, while the cargo process continued to completion. Linux /proc/93811/stat recorded cargo as a zombie with exit code 0. All PTY targets and doc-tests completed; see pty-exit-status.json. The known cloud auth-error case passed. Counts are in counts.json and full outputs in neighboring logs.

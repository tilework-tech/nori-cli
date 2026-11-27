#!/usr/bin/env bash
# usage: make-workflow.sh PR FULL_SHA HARNESS_SHA "suite ..." > rust-ci.yml
N=$1; S=$2; H=$3; SUITES=$4; S8=${S:0:8}; B=verify/pr-$N-$S8-native-host
TESTS=""; SEDS=""
for t in $SUITES; do TESTS="$TESTS --test $t"; SEDS="$SEDS
          sed -i.bak '/^#!\[cfg(target_os = \"linux\")\]\$/d' tui-pty-e2e/tests/$t.rs"; done
cat <<EOF
name: rust-ci
on:
  push:
    branches: ['$B']

jobs:
  checks:
    name: macOS interactive acceptance for #$N at $S8
    runs-on: blacksmith-6vcpu-macos-15
    timeout-minutes: 150
    defaults:
      run:
        working-directory: nori-rs
    env:
      CARGO_INCREMENTAL: "0"
    steps:
      - uses: actions/checkout@v6
        with:
          ref: $S
          persist-credentials: false
      - name: Verify source revision
        run: test "\$(git rev-parse HEAD)" = $S && git diff --quiet HEAD -- . && git rev-parse HEAD HEAD:nori-rs && uname -sm && sw_vers
      - uses: dtolnay/rust-toolchain@1.90
        with:
          targets: aarch64-apple-darwin
      - uses: dtolnay/rust-toolchain@nightly
        with:
          targets: aarch64-apple-darwin
      - name: PR PTY suites on macOS (Linux gate removed), two rounds
        run: |
          mkdir -p "\$RUNNER_TEMP/mac" target/mock-acp-out
          cargo +nightly build -p mock-acp-agent --profile ci-test --target aarch64-apple-darwin --artifact-dir target/mock-acp-out -Z unstable-options
          cargo build -p nori-cli -p nori-tui --bin nori --bin nori-tui --profile ci-test --target aarch64-apple-darwin
          export MOCK_ACP_AGENT_BIN="\$GITHUB_WORKSPACE/nori-rs/target/mock-acp-out/mock_acp_agent"$SEDS
          for round in 1 2; do cargo test --profile ci-test --target aarch64-apple-darwin -p tui-pty-e2e --no-fail-fast$TESTS > "\$RUNNER_TEMP/mac/pty-round-\$round.log" 2>&1 || true; grep -E "^test .* (ok|FAILED)\$|^test result" "\$RUNNER_TEMP/mac/pty-round-\$round.log" | sed "s/^/round \$round: /"; done | tee "\$RUNNER_TEMP/mac/pty-rounds.txt"
          git checkout -- tui-pty-e2e/tests
      - name: Release nori at $S8
        env:
          CARGO_PROFILE_RELEASE_LTO: "off"
          CARGO_PROFILE_RELEASE_CODEGEN_UNITS: "16"
        run: |
          cargo build --release --locked --bin nori --target aarch64-apple-darwin
          test "\$(git rev-parse HEAD)" = $S && git diff --quiet HEAD -- .
      - name: Acceptance harness (independent verifier)
        uses: actions/checkout@v6
        with:
          ref: $H
          path: acc
          persist-credentials: false
      - name: Interactive tmux scenarios on native macOS
        working-directory: acc/harness/attach-builtins-verify
        run: |
          brew install tmux bash >/dev/null
          tmux -V; bash --version | head -1; python3 --version
          NORI_BIN="\$GITHUB_WORKSPACE/nori-rs/target/aarch64-apple-darwin/release/nori"
          bash run-all.sh macos-$N-$S8 "\$NORI_BIN" "\$RUNNER_TEMP/mac" || true
          cat "\$RUNNER_TEMP/mac/macos-$N-$S8/checks.txt" || true
      - name: Upload macOS evidence
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: macos-pr-$N-$S8
          path: \${{ runner.temp }}/mac
EOF

"""Print a verify/pr-N-SHA8-native-acceptance rust-ci.yml pinned to a PR head and an acceptance-harness commit.

Usage: native_acceptance_workflow.py N SHA HARNESS_SHA "TEST CMD" SCENARIOS PROBES "OLD_SHA,MAIN_SHA" > .github/workflows/rust-ci.yml
PROBES and the comparison pair may be empty strings."""
import sys, textwrap
n, sha, harness, tests, scen, probes, extra = sys.argv[1:8]
short = sha[:8]
branch = f"verify/pr-{n}-{short}-native-acceptance"
extra_builds = [e for e in extra.split(",") if e]
build_extra = "".join(f"""
      - name: Optimized nori at {e[:8]} (comparison build)
        env:
          CARGO_PROFILE_RELEASE_LTO: "off"
          CARGO_PROFILE_RELEASE_CODEGEN_UNITS: "16"
        run: |
          git worktree add --detach "$RUNNER_TEMP/wt-{e[:8]}" {e}
          cd "$RUNNER_TEMP/wt-{e[:8]}/nori-rs"
          test "$(git rev-parse HEAD)" = {e}
          CARGO_TARGET_DIR="$RUNNER_TEMP/target-{e[:8]}" cargo build --release --locked --bin nori --target ${{{{ matrix.target }}}}
          mkdir -p "$RUNNER_TEMP/bin-{e[:8]}" && cp "$RUNNER_TEMP/target-{e[:8]}/${{{{ matrix.target }}}}/release/nori" "$RUNNER_TEMP/bin-{e[:8]}/nori"
""" for e in extra_builds)
perf = ""
if extra_builds:
    old, main = extra_builds
    perf = f"""
          python3 -I harness/perf/joined_open.py --build old-main-{old[:8]} "$RUNNER_TEMP/bin-{old[:8]}/nori" 0 --build main-{main[:8]} "$RUNNER_TEMP/bin-{main[:8]}/nori" 1 --build pr-{short} "$NORI_BIN" 1 --sections 64 150 170 --samples 3 --out "$RUNNER_TEMP/mac/joined_open.json" | tee "$RUNNER_TEMP/mac/joined_open.log" || true
          python3 -I harness/run.py --nori "$RUNNER_TEMP/bin-{main[:8]}/nori" --out "$RUNNER_TEMP/mac/main-control" --only {scen} | tee "$RUNNER_TEMP/mac/main-control.log" || true"""
probe_line = f"""
          python3 -I harness/feature_probes.py --nori "$NORI_BIN" --only {probes} --out "$RUNNER_TEMP/mac/probes.json" | tee "$RUNNER_TEMP/mac/probes.log" || true""" if probes else ""
wf = f"""name: rust-ci
on:
  push:
    branches: ['{branch}']

jobs:
  checks:
    name: macOS independent acceptance for #{n} at {short}
    runs-on: blacksmith-6vcpu-macos-15
    timeout-minutes: 150
    defaults:
      run:
        working-directory: nori-rs
    env:
      CARGO_INCREMENTAL: "0"
    strategy:
      matrix:
        include:
          - target: aarch64-apple-darwin
    steps:
      - uses: actions/checkout@v6
        with:
          ref: {sha}
          fetch-depth: 0
          persist-credentials: false
      - name: Verify source revision
        run: test "$(git rev-parse HEAD)" = {sha} && git diff --quiet HEAD -- . && git rev-parse HEAD HEAD:nori-rs && uname -sm && sw_vers
      - uses: dtolnay/rust-toolchain@1.90
        with:
          targets: ${{{{ matrix.target }}}}
      - uses: dtolnay/rust-toolchain@nightly
        with:
          targets: ${{{{ matrix.target }}}}
      - name: PR tests
        run: |
          mkdir -p target/mock-acp-out
          cargo +nightly build -p mock-acp-agent --profile ci-test --target ${{{{ matrix.target }}}} --artifact-dir target/mock-acp-out -Z unstable-options
          export MOCK_ACP_AGENT_BIN="$GITHUB_WORKSPACE/nori-rs/target/mock-acp-out/mock_acp_agent"
          {tests} 2>&1 | tee "$RUNNER_TEMP/pr-tests.txt"
          grep -E "^test result:" "$RUNNER_TEMP/pr-tests.txt"
      - name: Optimized nori at {short}
        env:
          CARGO_PROFILE_RELEASE_LTO: "off"
          CARGO_PROFILE_RELEASE_CODEGEN_UNITS: "16"
        run: |
          cargo build --release --locked --bin nori --target ${{{{ matrix.target }}}}
          test "$(git rev-parse HEAD)" = {sha} && git diff --quiet HEAD -- .
{build_extra}
      - name: Acceptance harness (independent verifier)
        uses: actions/checkout@v6
        with:
          ref: {harness}
          path: acc
          persist-credentials: false
      - name: Two-terminal acceptance on native macOS
        working-directory: acc
        run: |
          mkdir -p "$RUNNER_TEMP/mac"
          cp "$RUNNER_TEMP/pr-tests.txt" "$RUNNER_TEMP/mac/" || true
          brew install tmux >/dev/null
          tmux -V; python3 --version; uname -sm; sw_vers
          NORI_BIN="$GITHUB_WORKSPACE/nori-rs/target/${{{{ matrix.target }}}}/release/nori"
          shasum -a 256 "$NORI_BIN" > "$RUNNER_TEMP/mac/nori.sha256"
          python3 -I harness/run.py --nori "$NORI_BIN" --out "$RUNNER_TEMP/mac/scenarios" --only {scen} | tee "$RUNNER_TEMP/mac/scenarios.log" || true{probe_line}{perf}
      - name: Upload macOS evidence
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: macos-pr-{n}-{short}
          path: ${{{{ runner.temp }}}}/mac
"""
sys.stdout.write(wf)

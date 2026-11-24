# nori host TUI acceptance

Independent acceptance harness and evidence for the shared host TUI stack in
`tilework-tech/nori-cli`. All data is synthetic: three generated project roots
(`alpha/app`, `beta/app` with the same logical path, and a `laptop/app` decoy),
throwaway `NORI_HOME`s, and a scripted stdio agent server (`lantern_adapter.py`)
that stands in for Claude Code, Codex and OpenCode under their slugs.

## Layout

```
MATRIX.md                 requirements -> scenario checks -> status per commit
harness/run.py            scenario runner (S01-S15)
harness/lantern_adapter.py  synthetic ACP agent server with a frame ledger
harness/fixtures.py       synthetic homes, roots and [[agents]] config
harness/term.py           tmux PTY driver (private socket, raw byte log)
harness/wire.py           independent observer connection to the host
harness/proxy.py          TCP relay that can drop attach connections
harness/render.sh         renders .ansi captures to PNG with Ghostty Web
results/<sha8>/           results.json, summary.json, per-scenario captures and ledgers
```

## Reproduce

```bash
# in a nori-cli checkout at the commit under test
cd nori-rs && cargo build --bin nori
# in this branch
python3 -I harness/run.py --nori /path/to/nori-rs/target/debug/nori --out results/<sha8>
harness/render.sh results/<sha8>          # optional PNGs (needs bun + Chrome)
python3 -I harness/run.py --nori ... --out /tmp/x --only S03a,S08   # subset
```

Requirements: Linux, tmux, python3, the `nori` debug build. Each scenario
starts its own host under `/tmp/nori-host-tui-acceptance/<id>` and stops it.

## Per-PR verification

For each stack PR the verifier builds the PR head, runs the full matrix into
`results/<sha8>/`, re-runs the repository suites (`nori-cli --test host_lifecycle
--test managed_recovery`, `tui-pty-e2e --test host_attach --test attached_external_cancel`),
reads Linux CI for the head and an exact-commit native macOS run on
`verify/pr-<N>-<sha8>-native-host`, and posts a commit-specific verdict on the PR.

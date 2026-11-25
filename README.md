# nori host TUI acceptance

Independent acceptance harness and evidence for the shared host TUI stack in
`tilework-tech/nori-cli`. All data is synthetic: three generated project roots
(`alpha/app`, `beta/app` with the same logical path, and a `laptop/app` decoy),
throwaway `NORI_HOME`s, and a scripted stdio agent server (`lantern_adapter.py`)
that stands in for Claude Code, Codex and OpenCode under their slugs.

## Layout

```
MATRIX.md                 requirements -> scenario checks -> status per commit
harness/run.py            scenario runner (S01-S20)
harness/lantern_adapter.py  synthetic ACP agent server with a frame ledger
harness/fixtures.py       synthetic homes, roots and [[agents]] config
harness/term.py           tmux PTY driver (private socket, raw byte log)
harness/wire.py           independent observer connection to the host
harness/proxy.py          TCP relay that can drop (hard) or blackhole (silent) attach connections
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

## Attach replay performance (#769 / #770)

`harness/perf/` measures `nori attach` opening a saved host session with a
synthetic `kiln_agent.py` history in a draining tmux pane, and stitches the
full scrollback to prove exactly-once ordering. `replay769.py` runs base/fix
pairs (`open` and failed-session `recover` modes); `summarize.py` tabulates
them; `run_linux*.sh` hold the Linux matrix (run with `bash`). Results and the verdict are in
`results/769-0e673ef4/`.

## Shared attach stack (#776, #777, launcher)

- `harness/verifier/` holds verifier-only Rust tests (`inject.sh` copies them into a
  verifier tree), the 13-envelope host replay fixture, and
  `verifier-joined-app.patch`, which lets `nori attach` run `run_joined_app` when
  `NORI_VERIFY_JOINED_APP` is set, before the launcher cutover exists.
- `run.py` J01–J06 drive the joined App with sessions seeded by `nori host new`.
  Run them against a wrapper that sets `NORI_VERIFY_JOINED_APP=1`.
- `harness/perf/joined_open.py` compares long-history open and first key for
  attach vs the joined App, and audits the whole inline scrollback.
- `harness/diffstates.py OLD NEW` lists only checks whose pass/fail changed.
- Verdicts: `results/attach-776-b2562f5e/`, `results/attach-777-20051fcf/`.
- `harness/feature_probes.py --nori BIN --out FILE [--only queue,history,prompts,goal,context]`
  probes the host-backed feature methods (#778–#782) on the wire with a controller
  and a watcher. K01–K04 in `run.py` cover the same features in plain `nori`. The
  lantern `http-mcp` control switch makes the agent advertise HTTP MCP, which host
  goal management requires. Subscribe with `{"session_id", "cursors": {id: n}}`; the
  host ignores `sessionId`/`cursor` and falls back to the selected session.

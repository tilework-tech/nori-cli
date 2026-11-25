# PR #773 complete terminal-palette evidence

Verbatim copy of `scripts/terminal-palette/` from nori-cli PR #773 head `a5daa5973c9f6ff680cfb9636167bb3d68450375`, before the bulk results moved out of the feature diff, plus the before/after tmux captures from `proof/terminal-palette/` at `8ed3c667bc2035e79229667a91feeefb043209d3` (`captures/`). Files are byte-identical to those commits.

## Measured builds

| Label in `build` column / `binaries` | Source | Binary SHA-256 |
|---|---|---|
| `base`, `baseline` | Production tree of `8099204975768d0b0d17e7bd21f13cfbd48ddb59` | `4b7fe6aa295fa529df92a6d55bd0abbe63cca04f7de6d31e2215c7825ec6e38d` |
| `fixed` | Candidate local commit `5d03ec6c9` (production tree later published as #773) | `bbc55a1547dc4e07f4223a00654144e9bbaa4f6f849672e228148bb76aa85845` |
| `next.6` | Published `nori-v0.30.0-next.6` Linux x86-64 musl artifact | see PR #770 manifest |

Builds: `cargo +1.90.0 build --release --locked --target x86_64-unknown-linux-musl -p nori-cli`.

## Machine

One Modal x86-64 Linux host, kernel 7.3.4 (`Linux-7.3.4-x86_64-with-glibc2.36`), Python 3.11+, 120×40 controlling PTY drained continuously, warm optimized binaries, fresh homes, local synthetic agents. Captures use a real tmux pane rendered with Ghostty Web.

## Fixtures

Synthetic only: two ready host sessions seeded from PR #770's `scripts/attach_perf` fixtures (one Markdown section, one completed tool, one filler conversation). The cloud case uses a synthetic `NORI_HANDROLL_BIN` advertising three saved sessions.

## File index

| File | Contents |
|---|---|
| `samples.csv` | 333 raw rows from `measure.py`: base and fixed × entry point × terminal response mode (`immediate`, `delayed` 150 ms, `missing`, plus `background-only`, `silent-all`, `device-silent`, exit and handoff cases), 5 or 3 samples each; 9 `next.6` attach rows. All `ms_` columns start at process launch. |
| `handoffs.csv` | Ctrl+G editor and Ctrl+Z/`fg` job handoff, 3 samples per build. |
| `startup-764.json`, `startup-764-repeat.json` | Two runs of the unmodified PR #764 eight-gate benchmark, 20 samples per slot (baseline, baseline, fixed). |
| `captures/before.{png,txt}`, `captures/after.{png,txt}` | Attach pane at about 104 ms after launch with palette queries unanswered; δ typed at 35 ms. |
| `README.md` | Full write-up and tables that accompanied these results. |

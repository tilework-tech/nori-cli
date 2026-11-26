# Independent acceptance: #792 at 3350ba87 (instruction file names in /context)

All data is synthetic. No feature branch was edited. `3350ba87` is one commit on `main` 66264b5d. Release builds of `3350ba87` and landed `main` 85da3524 (control).

## Verdict: pass on Linux and native macOS

R09 (one `CLAUDE.md` in the session folder) and the new R09b run `/context` through the real `nori attach` UI from a remote terminal with its own home, after Alt+T. R09b puts five instruction files under long paths that share a prefix: the host home's `~/.claude/CLAUDE.md`, a parent folder's `CLAUDE.md`, and the session folder's `CLAUDE.md`, `CLAUDE.local.md` and `.claude/CLAUDE.md`. The client home has a decoy `.claude/CLAUDE.md`.

| Check | `3350ba87` 120 cols | `3350ba87` 60 cols | `main` 120 / 60 |
|---|---|---|---|
| R09: the row shows `CLAUDE.md` | pass (`/tmp/…/R09/…/app/CLAUDE.md`) | – | **fail** (`…/beta/app/C...`) |
| Five instruction rows | pass | pass | pass |
| All five labels are distinct | pass | pass | **fail** (four read `…/roots/beta/ap...`) |
| Each label ends in its own file name | pass | pass | **fail** |
| Both `.claude/CLAUDE.md` rows keep `.claude/` | pass | pass | **fail** |
| The client decoy is absent | pass | pass | pass |

Rows on `3350ba87` (`scenarios/R09b/rows-*.json`):
```
~/.claude/CLAUDE.md
/tmp/…/northern-estuary-soundings/CLAUDE.md
/tmp/…/tidewater-beacon-survey/CLAUDE.md
/tmp/…/tidewater-beacon-survey/CLAUDE.local.md
/tmp/nori-host-tui-acceptance/…/.claude/CLAUDE.md
```

- Scenarios: `3350ba87` 15/15, `main` 8/15 (`main-85da3524-control/`). PNGs: `scenarios/R09b/context-{120,60}.png` and the same under the control.
- Low, not blocking: the session folder's `.claude/CLAUDE.md` keeps the first path segment and drops its folder, so the row does not say which folder it belongs to (only that it differs from `~/.claude/CLAUDE.md`). Keeping the parent of `.claude` would make it `…/tidewater-beacon-survey/.claude/CLAUDE.md`.
- Linux CI on the head passes.

## Native macOS

macOS 15.7.4 arm64, [run 45726825204](https://github.com/tilework-tech/nori-cli/actions/runs/45726825204), branch `verify/pr-792-3350ba87-native-acceptance`, checked out at `3350ba87` (the `nori-rs` tree matches the head, `8bc50fa6`). Harness `7503ab40`. Evidence in `macos/`.

- R09 and R09b: 15/15. The labels start with `/private/tmp/…` or `/private/…`; all five are distinct and end in their own file names.
- The PR's `context_view` unit tests: 8/8.

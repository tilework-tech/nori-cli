# Independent acceptance: selector stack #795, #797, #798, #799

All data is synthetic: an `orchard` project with `pear_ledger.rs` / `plum_rows.rs`, 18 host sessions with fruit-orchard titles, throwaway homes under `/tmp/vsel/runs`, and the repo's `mock_acp_agent`. Approvals come from the mock agent (`MOCK_AGENT_REQUEST_PERMISSION`, command `rm -rf ./synthetic-build`, never run). No feature branch was edited.

Scripts: `harness/selectors/` (`drv.py`, `suite.py`). Run: `python3 harness/selectors/suite.py <bins> <sha8> <out>`.

## Builds under test

| Label | Commit | What it is |
|---|---|---|
| `66a76859` | `66a76859599e` | #794 head, baseline before #795 |
| `c6cef012` | `c6cef0121ba3` | **#795** head |
| `27ccdfda` | `27ccdfda8383` | #791 head (on #795), baseline before #797 |
| `d4239329` | `d4239329b451` | **#797** head |
| `575e4af0` | `575e4af005d1` | **#798** head |
| `7a797061` | `7a797061ff6e` | **#799** head (stack tip) |

Debug builds, Rust 1.90.0, Linux x86_64, tmux 3.3a. Each scenario runs the real `nori` binary in its own tmux pane.

- **Shaded terminal**: the pane has `window-style bg=#141414`, so tmux answers nori's OSC 11 query and `COLORTERM=truecolor` is set.
- **Unshaded terminal**: no pane background, so the OSC 11 query gets no reply.

Cell colours and bold are read by interpreting each ANSI capture with `pyte`. The PNGs come from Ghostty Web, which draws `▏` and `▕` as blank cells (like `┄`), so the `.txt` captures are authoritative for the rails.

## Verdicts

| PR | Head | Verdict | Asserted checks |
|---|---|---|---|
| #795 focus style | `c6cef012` | **pass** | 109/109 |
| #797 short prompts | `d4239329` | **fail (shaded terminals only)** | 127/129 |
| #798 `(current)` / `(on screen)` | `575e4af0` | **pass** for its own change. It inherits the #797 failure | 136/138 |
| #799 `/model` search, shared keys | `7a797061` | **pass** for its own change. It inherits the #797 failure | 147/149 |

The baselines pass every check that applies to them: `66a76859` 37/37 and `27ccdfda` 109/109. Checks for behaviour from a later PR are recorded as `info` and are not asserted (`results.json` per build).

### Regression in #797: in shaded terminals the focused choice in a short prompt is barely distinguishable

Short prompts sit on the composer panel shade, `blend(white, bg, 10%)`. The focus shade from #797 is `Theme.selected = relative_surface(bg, 10)`, which is the same colour (`#2b2b2b` on a `#141414` terminal). With #797:

- the focused row's background equals the rest of the prompt
- the label is no longer green or bold
- the marks are not bold, because the terminal is shaded

The only difference left is the thin green `▏ › ▕`. Before #797 (`27ccdfda`), the whole focused choice was bold green.

| Cell (row "1. Allow (y)") | `27ccdfda` (before) | `d4239329` (#797) |
|---|---|---|
| focused row fg / bg / bold | green / `2b2b2b` / bold | label default / `2b2b2b` / not bold; marks green, not bold |
| unfocused row "2. Reject" bg | `2b2b2b` | `2b2b2b` |

Minimal user-visible case:

1. Use a dark terminal that reports its background and supports truecolor.
2. Have the agent ask permission to run any command.
3. Press `j` / `k`. The highlighted choice shows no shading and no bold, only a thin green pointer and rails.

The same applies to `/approvals` → "Approve all" (confirmation prompt). Unshaded terminals pass, because there the marks are bold.

Evidence: `27ccdfda/P1-permission-shaded/01-prompt.{txt,ansi,png}` against `d4239329/P1-permission-shaded/01-prompt.{txt,ansi,png}`, and `*/P2-approve-all-shaded/02-confirm.*`.

## What passed (stack tip `7a797061`, same on each PR's own head)

| Area | Result |
|---|---|
| L1 `/settings`, shaded and unshaded, 120 cols | Shaded: green rails and `›`, row shaded `#2b2b2b` against default neighbours, marks not bold. Unshaded: no background, marks bold. `▏› 1.` pointer beside the number, no `◉`. `j`/`k` move. `x` does not filter. `f`, Ctrl+F and `/` each open `⌕`, typing filters, Esc ends the search. Footer: `↑↓/j/k move  / search  enter select  esc close` |
| L2 `/settings` at 60 and 40 cols | Both rails on every focused line, and both colour modes are correct |
| L3 `/model` (2 values) | `Mock Default Model (current)`. Footer offers `/ search`. `f` + `fast` leaves only Mock Fast Model (#799) |
| L3 `/config`, Thought Level (3 values), `/agent` | No search (`f` does nothing, no `/ search` in the footer). `Medium (current)` is focused when opened. `/agent` shows `(current)` and keeps `shift-tab rec on` |
| P1 permission prompt | An Enter sent the moment the prompt appeared was ignored (grace delay kept), and a later Enter was accepted. Header `Command: rm -rf ./synthetic-build` kept. `j`/`k` move, `f` does not search. Label stays the normal colour |
| P2 approve-all confirmation | Red warning "…nothing else stops a harmful request." kept. Footer `↑↓/j/k move, enter confirm, esc go back` |
| U1 `/`, `@`, `$` popups | Identical cell-for-cell (text, fg, bg, bold) on all six builds |
| L4 `nori attach` list and `/sessions`, 18 sessions, shaded and unshaded | Marks correct in both modes. `j`/`k`, End/Home, `f`/Ctrl+F/`/` search. `q` does not filter. The opened session reads `Plum row cleanup (on screen)`, with no `(current)` (#798), still at 40 cols. Rails at 60 and 40 cols |
| L5 `nori resume`, 18 saved sessions | `j`/`k`, Home/End (#799), `f`/Ctrl+F/`/` search, stray letter ignored |

## Observations (not failures)

1. **Already in the #794 baseline:** at 32 cols the session table is wider than the terminal, so the status column and the right rail are cut off. The rails add one cell each side.
2. **Also on every build, baseline included:** after an inline resize from 60 to 40 cols, the earlier, now-clipped copy of `/settings` stays above the redrawn one.
3. Short prompts ignore Home/End/PgUp/PgDn. `list_selection_view.rs` maps `First | Last | PageUp | PageDown` to no-op. `docs/tui.md` says "In every picker … Home/End". Short prompts may not count as pickers, so the owner should decide.
4. During a search in the `nori attach` / `/sessions` list, `j` filters (correct), but the footer still reads `↑↓/j/k move`. `/settings` switches its footer to `↑↓ move  type filter … esc stop search`.
5. The `nori resume` footer says `↑↓/j/k browse`, not `move`.
6. Leaving a search with Esc returns focus to the first row, not the row focused during the search.

## Not covered

- `/api-key` prompts: the mock agent takes no API key, so the prompt does not open.
- Delegation prompts and Shift+Tab inside a prompt.
- Native macOS and real terminal emulators (kitty, Ghostty, WezTerm). Shading came from tmux's OSC 11 answer.
- `cargo test` / `just test e2e` were not rerun.

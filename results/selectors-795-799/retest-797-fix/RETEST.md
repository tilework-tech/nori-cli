# Retest: the shaded short-prompt finding on #797, #798 and #799

This checks the fix for the failure recorded in `../VERDICT.md`. It uses the same harness (`harness/selectors/suite.py`), the same synthetic orchard data and mock approvals, and tmux 3.3a on Linux x86_64 with debug builds.

The suite gained two checks for short prompts (permission prompt and the "Approve all" confirmation), both run in shaded terminals:

- **Full-row contrast.** Every cell between `▏` and `▕` must share one background. That background must differ from the default and from the panel cell just outside the left rail.
- **Focus movement.** The marks and full-row contrast are checked again after `j` moves focus to the second choice.

## Builds

| Label | Commit | Role |
|---|---|---|
| `d4239329` | `d4239329b451` | Previous failing #797 head (control) |
| `0f47a50b` | `0f47a50b0cad` | **#797** head, with the fix |
| `f2630c0e` | `f2630c0ed924` | **#798** head |
| `d387c9ec` | `d387c9ec4ef1` | **#799** head (stack tip) |

The #791 base (`27ccdfda`) is unchanged and is an ancestor of all three heads.

## Verdicts

| Head | Verdict | Checks passed |
|---|---|---|
| control `d4239329` | still fails, as expected (8 shaded short-prompt checks) | 166/174 |
| #797 `0f47a50b` | **pass** | 174/174 |
| #798 `f2630c0e` | **pass** | 174/174 |
| #799 `d387c9ec` | **pass** | 174/174 |

Focused row of a short prompt in a shaded terminal (`#141414` background):

| Head | Prompt | Panel bg | Focused row bg (every cell between rails) | Marks | Label |
|---|---|---|---|---|---|
| `d4239329` | permission, before and after `j` | `#2b2b2b` | `#2b2b2b` (same as the panel) | green, plain | default |
| `0f47a50b` | permission, before and after `j` | `#2b2b2b` | `#434343` | green, plain | default |
| `d387c9ec` | Approve all, before and after `j` | `#2b2b2b` | `#434343` | green, plain | default |

In unshaded terminals, every head has no row background and bold green rails and pointer.

All other checks from the first round passed unchanged on all three new heads:

- the grace delay (an Enter sent the instant the prompt appears is ignored)
- the command header and the red approve-all warning
- the `/`, `@` and `$` popups
- `(current)` and `(on screen)`
- `/model` search with 2 values
- no search on short lists
- j/k, Home/End, and `f`, `/` and Ctrl+F search in `/settings`, `nori attach` / `/sessions` and `nori resume`
- 60 and 40 column rails

## Evidence

- `<head>/P1-permission-shaded/01-prompt.*` and `01b-prompt-after-j.*`
- `<head>/P2-approve-all-shaded/02-confirm.*` and `02b-confirm-after-j.*`
- `<head>/P1-permission-unshaded/01-prompt.*`
- `<head>/results.json` and `run.log`

PNGs come from Ghostty Web, which draws `▏ ▕` as blank cells. The `.txt` and `.ansi` captures are authoritative for the rails.

## Limitations and open notes

- The first round's owner notes still apply on these heads, and none of them is a failure: the session table overflows at 32 columns (as on the baseline), short prompts ignore Home/End, the footer during a `/sessions` search, and the "browse" wording in `nori resume`.
- Not covered: `/api-key` and delegation prompts, Shift+Tab inside a prompt, light terminal backgrounds (the fix's unit test covers `#f0f0f0`; this retest did not drive it), native macOS and real terminal apps, and the `cargo test` / e2e suites.

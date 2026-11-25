# Independent acceptance: #782 at bd7d9f54 (per-session context) and the cumulative stack

## Verdict: pass, one low note. Stack-wide: #778 F1 stands.

- **Context probes:** 7 of 7 (`probes-context-claude-md.json`).
  - A watcher may read; the reply has `timeline`, `instructions` and `usage`.
  - `CLAUDE.md` is found in the session's own folder on the host and not listed for another session.
  - `skipInstructions` omits it.
  - An unknown session gets `-32002`.
- **Plain `nori` `/context`** now lists the folder's instruction file (about 10 tokens), where `main` listed none (`captures/context.png` vs `captures/context-main.png`).
- **Cumulative probes at this head:** 53 of 59 pass (`probes.json`). The failures are exactly #778 F1 (4 checks), #779 L1, and one probe that used the wrong instruction file (corrected in `probes-context-claude-md.json`).
- **Suites at this head:**
  - `nori-cli --test host_lifecycle`: 177 passed (queue 7, history 5, prompts 6, goal 6, context 6).
  - `nori-tui`: 1,909 passed.
  - `tui-pty-e2e`: 310 passed. 0 failed.
- **Regression matrix:** the only change against `main` is S34m `control.no_host_requests`, which #779 and #780 introduce (`s34m-bisect/`).

Low:
- **L1:** the `/context` instruction row truncates long paths at the end, which hides the file name (`…/roots/alpha...  ~10`).

Not covered: native macOS, and the joined-surface `/goal` and `/context` (need the launcher).

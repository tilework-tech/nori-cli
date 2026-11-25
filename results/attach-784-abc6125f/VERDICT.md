# Independent acceptance: #784 at abc6125f (re-check of c8f3ee3b)

All data is synthetic. No feature branch was edited. `abc6125f` adds three commits to `c8f3ee3b`: watchers can't interrupt and joined-host notices are fixed, the transcript pager closes before streaming replay, and two attach tests wait for what they assert. The `c8f3ee3b` verdict (`../attach-784-c8f3ee3b/VERDICT.md`) has the method and baselines.

## Verdict: two terminals pass on Linux and native macOS. M1 and M2 remain; L1–L3 remain.

| | Linux | macOS 15 arm64 ([run 45705230730](https://github.com/tilework-tech/nori-cli/actions/runs/45705230730), `verify/pr-784-abc6125f-native-acceptance`) |
|---|---|---|
| J01–J06, R01–R03, S09, S16 | all pass (`joined/`) | all pass (`macos/`) |
| Attach PTY suites, 3 rounds | – | 108/108 (the 4 earlier failures are fixed) |
| Full PTY suite | – | 1,972 passed, 0 failed |

- **M1 still open.** Median ms, first row / newest reply (`perf/joined_open.json`). Linux: 90 / 439 at 64 sections, 87 / 1,683 at 150 and 88 / 2,128 at 170. Main: 55 / 70, 55 / 87 and 55 / 102. macOS: 63 / 346 and 64 / 1,301.
- **M2 still open.** After Ctrl+L, 7–11 of 150–170 sections are written twice (`perf/joined_open.json`, `audit.dup_marks`).
- **L1 still open.** A refused `/new`, `/compact` or `/agent` clears the draft (S35b, S35c, S35h; Linux and macOS).
- **L2 still open.** The agent-advertised `/review` is rejected as unrecognized (S35b).
- **L3 still open.** With no session, `@` lists the client's own files (S41c, Linux and macOS). After Alt+S, `@` keeps the first session's folder (S41a, Linux).

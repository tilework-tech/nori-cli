# PR #749 at `3b2010d4` (previous `5fa7be45`)

Verdict: pass (inherits the open #746 attach finding from its base).

- Matrix: 282/319, identical per check to `5fa7be45` on S01-S33b; no regression against #748.
- S33c (new): a prompt typed ~1.4 s after `/new` reaches the new server exactly once and is answered (3/3 at both revisions).
  The synthetic agent server prepares too fast to open the race `3b2010d4` fixes; the repository `host_new_session` test covers it.
- Linux CI run 45348026352 green. Native macOS run 45348041985 on `verify/pr-749-3b2010d4-native-host` (pinned) green.
- `/new DIR` in plain `nori` still ignores DIR (later cwd layer), so folder inheritance remains unobservable.

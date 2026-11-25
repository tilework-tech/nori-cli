# Independent acceptance: #780 at 2f11ea33 (host custom prompts)

## Verdict: pass, one low note

Wire probes pass 12 of 12:
- A watcher lists the host's prompts with their bodies, and client-home prompts are not listed.
- A script runs on the host in the session's own folder (beta, not the launch folder) and receives its 3 args verbatim, including `a b`, `$(touch …)` and `; touch …`. No injection canary was created.
- A watcher's run gets `-32016`.
- `../outside`, `../outside.sh`, an absolute path, a missing name, and a markdown prompt run as a script are all refused, and nothing outside `commands/` ran.

Plain `nori` K02: the popup lists the host prompt, markdown expands and sends, and script output is sent exactly once.

- **L1:** as in #779, `nori resume --last` also sends `_nori/host/prompts` (S34m).

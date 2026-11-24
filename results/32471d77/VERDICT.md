# #750 `32471d77` (feat/host-startup-picker), parent #749 `541c65f6`

**Pass.** No regressions; every S34 check passes; two parent gaps are closed.

- S34a-m 77/77 (parent 60/77). Host path proven by `host-requests.txt` per scenario, with S34m as the in-process control.
- Exactly-once delivery for typed, positional, piped and piped+`--stdin` input given while `initialize` is held.
- Refused startup keeps typed and positional input unsent and in the composer; Enter retries once (parent drops it).
- `/resume` during startup reuses the one startup adapter: one `session/load`, no `session/new`.
- Fork after `/new` is now followed (parent: `Session forked.` never shown); delegated child results still arrive.
- Full matrix: 317/353 outside S34, identical check-by-check to the parent baseline `e86f3803`.
- Linux CI 45373737626; native macOS 45373745120 (author) and 45379653626 (verifier, adds the host suites).

Observations (not blocking, already known to the owner): no "waiting" marker while held input waits; the parent
shows the bare refusal reason while #750 prefixes `Couldn't activate Lantern Claude:`.

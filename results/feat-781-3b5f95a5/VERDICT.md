# Independent acceptance: #781 at 3b5f95a5 (per-session goal from any terminal)

## Verdict: pass

Wire probes pass 12 of 12. The synthetic agent advertises HTTP MCP through the `http-mcp` control switch; without it the host correctly refuses with "goal management is unavailable".
- Goals start empty and are kept per session.
- A watcher subscribed to the session receives `goal_changed` live and can read the goal.
- A watcher's set gets `-32016`.
- Pause and resume work, and clearing returns `null`.
- A late joiner's list row carries the goal.
- An unknown session gets `-32002`.

Setting a goal starts a continuation turn, as expected for goals.

Plain `nori` `/goal` couldn't be exercised: the synthetic agent has no goal support in the TUI path, and main behaves the same.

Not covered: the goal UI on the joined surface (needs the launcher).

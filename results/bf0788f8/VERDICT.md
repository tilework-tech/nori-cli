# #753 `bf0788f8` (registry: nori-monorepo #278 `c62bcc22`) — independent verdict

**Pass for CLI #753 `bf0788f8` and registry #278 `c62bcc22`.**

Tested heads: CLI #753 `bf0788f89a2d7ff89ebfd2ed04f588052ce14f29` on #752
`d4c1353e39d889645bdd7eab943ae1e116c60e1f`; first pass at `29e18411` (same
behaviour; `bf0788f8` only changes the folder-limit constant from `usize` to `i32`).

## Registry (#278)
At `a0d3ba51` the `_nori/session/find_paths` row gave params as `{session_id?, query, kind, limit?}`.
The host takes `sessionId` (`FindPathsParams` is `camelCase` with `deny_unknown_fields`;
the schema, TypeScript and `docs/remote-control.md` agree). With the registry's spelling
the host answers `-32602 unknown field session_id, expected one of sessionId, query, kind, limit`
(S41g, `results/bf0788f8-s41/S41g/wire-results.json`). Every other claim in the row
was observed true: root without a session is the host folder, links out of the root are
dropped, an empty file query gives no matches, `~/` folders come from the host home,
limit 0 is clamped to 1 and 100 is accepted, an unknown session gives `-32602`, watchers
may call it, and the capability is advertised. `_nori/session/new` resolves `~/x` with the
host home and `docs` against the host folder; `~nobody/x` is refused (`-32000`).

The implementer independently respelled the row at `c62bcc22` (`sessionId`, camelCase,
unknown fields refused) and added detail. Each added claim was checked on the wire at
`bf0788f8`: `session_id` refused with `-32602`, `.git` skipped, dot-folders only when the
typed name starts with `.`, folder names with whitespace skipped, malformed `kind` gives
`-32602`. The client column is accurate (`app/file_mentions.rs` sends through `HostFileSearch`).

## Real-terminal results (Linux, tmux, synthetic agent servers)
See `MATRIX.md`, section #753. S41: 49/49 at `bf0788f8` (S41g extended for `c62bcc22`) against 8/45 at `d4c1353e`.

## Inherited, not caused by #753
- macOS `host_attach` (ungated, parallel load, 10 rounds): busy Enter fallback 0/10,
  watcher permission answer 0/10, two-terminal takeover 5/10. Baselines: #751 run
  45433284870 0/0/7, #752 run 45429424837 0/0/8. Same tests, cause still unknown; 5/10
  is within the spread seen so far but worth watching.
- `tui/src/attach/docs.md` says older attach builds "fail on ApprovalModeChanged, so they
  cannot follow a host new enough to send it". Since #752 `d4c1353e`, older clients follow
  the host (S38: 24/24 with a `d4c1353e` client; 22/24 with `5cb13fcd`). The line came in with #752.

## CI coverage at `bf0788f8`
- Linux CI 45442798328: success; runs `host_attach` (19), `host_file_mentions` (3), `host_lifecycle` (138).
- Implementer native run 45444043612: success. Its main PTY step runs `host_attach` with
  0 tests (Linux gate); the three attach folder tests ran ungated 15/15, `host_file_mentions` 3/3 plus 5 rounds.
- Verifier native run 45445353172 (exact source checked): standard steps pass; ungated load step
  fails only on the three inherited tests above.

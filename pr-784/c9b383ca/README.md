# PR #784 proof: `nori attach` with two (and three) terminals

- Binary: `cli/nori-rs/target/debug/nori` at revision `c9b383ca`
- Grid: 120x40 for every capture; rendered with ghostty-web 0.4.0 via `tui-capture-with-ghostty-web`
- Data: synthetic only. `NORI_HOME=CODEX_HOME=/tmp/proof784/home`, cwd `/tmp/proof784/home/project` (trusted), `NORI_NO_ANALYTICS=1`,
  `MOCK_AGENT_RESPONSE="Synthetic orchard survey: 42 cedars, 7 maples."`, agent `mock-model`.
- Host: `nori --agent mock-model host --port 0` (daemonized on `ws://127.0.0.1:<port>/acp`); session id from `nori --agent mock-model host ls --json`.
- Terminals: A = `nori --agent mock-model attach`; B = `nori --agent mock-model attach <ID>`;
  C = `nori --agent mock-model attach <ID> --host ws://127.0.0.1:47784/acp --token-file $NORI_HOME/host/token` through `/tmp/proof784/relay.py 47784 <hostport>`.

| Capture | Terminal | Keys / commands | Proves |
|---|---|---|---|
| `01-picker-empty` | A | `attach` (no id) | Empty host opens the session list `Sessions 0` on the alternate screen |
| `02-controller` | A | `Esc`, `/new` `Enter`, `Survey the synthetic orchard` `Enter` | New session on the host; hint row `controlling · project · mock-model · ready · Approvals: Agent · ctrl+d detach`; mock answer rendered |
| `03-watcher-refused` | B | `attach <ID>`, `Synthetic watcher draft` `Enter` | Second terminal joins as `watching · read-only`; submit refused with an Alt+T hint; draft kept in composer |
| `04-takeover-b` | B | `M-t` (Alt+T) | B becomes `controlling`, draft still present |
| `05-takeover-a` | A | (none) | A demoted to `watching · read-only` with "Another terminal took control" notice |
| `06-inline` | B | `C-u`, `/screen` `Enter` | Switches to inline mode (alt screen off); history printed exactly once in scrollback |
| `07-fullscreen-again` | B | `C-l` | Back to full screen (alt screen on) with the transcript |
| `08-reconnecting` | C | kill relay | Hint row `RECONNECTING · … · draft kept`; loss notice in transcript |
| `09-reconnected` | C | restart relay on 47784 | "Reconnected to the host"; hint row back to `watching · read-only`; history shown once |
| `10-detached` | A | `C-d` | Attach exits 0 to the shell |
| `11-host-still-running` | shell | `host ls`, `host ls --json` | Session still listed (`ready`) after A detached; host not closed |

Cleanup: `nori --agent mock-model host stop` ("Host stopped."), then `tui-stop` on every session.

Note: `capture-session` writes a trailing newline after row 40 of `screen.ansi`, which makes ghostty-web scroll one row
and drop terminal row 1 from the PNG. Every `screen.png`/`metadata.json` here was re-rendered with `render-ansi.ts`
from its own `screen.ansi` minus that final newline; `screen.txt` and `screen.ansi` are the unmodified captures.
